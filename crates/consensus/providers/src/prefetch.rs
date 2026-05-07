//! Online data-availability prefetching for derivation.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use alloy_primitives::{Address, Bytes};
use async_trait::async_trait;
use base_common_genesis::RollupConfig;
use base_consensus_derive::{
    BlobProvider, ChainProvider, DataAvailabilityProvider, EthereumDataSource, PipelineError,
    PipelineErrorKind, PipelineResult,
};
use base_protocol::BlockInfo;
use futures::{StreamExt, stream::FuturesUnordered};
use tokio::task::JoinHandle;
use tracing::debug;

use crate::Metrics;

/// The maximum number of useful completed L1 data prefetch results retained locally.
pub const PREFETCH_BUFFER_CAPACITY: usize = 8;

/// The maximum number of empty completed L1 data prefetch results retained locally.
pub const PREFETCH_EMPTY_BUFFER_CAPACITY: usize = 512;

/// The maximum number of L1 blocks a worker scans while filling the useful buffer.
pub const PREFETCH_SCAN_BLOCK_LIMIT: usize = 512;

/// The maximum number of concurrent L1 data-availability fetches in one prefetch worker.
pub const PREFETCH_SCAN_CONCURRENCY: usize = 16;

/// Completed data-availability items for one L1 block and batcher address.
pub type PrefetchedData = (BlockInfo, Address, VecDeque<Bytes>);

/// Completed empty data-availability metadata for one L1 block and batcher address.
pub type EmptyPrefetch = (BlockInfo, Address);

/// The data collected by an L1 data-availability prefetch task.
pub type PrefetchResult = PipelineResult<PrefetchedData>;

/// A streamed result from the L1 data-availability prefetch worker.
pub type PrefetchMessage = PrefetchResult;

/// An active L1 data-availability prefetch worker: first block, last block, batcher, task.
pub type PrefetchWorker = (u64, u64, Address, JoinHandle<()>);

/// A bounded lookahead wrapper for [`EthereumDataSource`].
#[derive(Debug)]
pub struct PrefetchingEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Clone + Debug,
    B: BlobProvider + Send + Clone + Debug,
{
    /// The synchronous fallback source used for the block currently requested by the pipeline.
    pub source: EthereumDataSource<C, B>,
    /// Chain provider clone used by the background prefetch task.
    pub prefetch_chain_provider: C,
    /// Blob provider clone used by the background prefetch task.
    pub prefetch_blob_provider: B,
    /// Rollup config used to construct per-task data sources.
    pub rollup_config: Arc<RollupConfig>,
    /// Completed useful prefetched data, keyed by block and batcher address.
    pub prefetched: VecDeque<PrefetchedData>,
    /// Completed empty prefetched block metadata, keyed by block and batcher address.
    pub empty_prefetched: VecDeque<EmptyPrefetch>,
    /// Receiver for streamed prefetch worker results.
    pub prefetch_rx: Option<Arc<Mutex<Receiver<PrefetchMessage>>>>,
    /// Active prefetch worker metadata.
    pub prefetch_worker: Option<PrefetchWorker>,
    /// Last L1 block number and batcher address requested by the pipeline.
    pub last_request: Option<(u64, Address)>,
}

impl<C, B> PrefetchingEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Sync + Clone + Debug + 'static,
    B: BlobProvider + Send + Sync + Clone + Debug + 'static,
{
    /// Creates a new prefetching source from an active source and provider clones.
    pub const fn new(
        source: EthereumDataSource<C, B>,
        prefetch_chain_provider: C,
        prefetch_blob_provider: B,
        rollup_config: Arc<RollupConfig>,
    ) -> Self {
        Self {
            source,
            prefetch_chain_provider,
            prefetch_blob_provider,
            rollup_config,
            prefetched: VecDeque::new(),
            empty_prefetched: VecDeque::new(),
            prefetch_rx: None,
            prefetch_worker: None,
            last_request: None,
        }
    }

    /// Creates a new prefetching source from provider parts.
    pub fn new_from_parts(provider: C, blobs: B, cfg: Arc<RollupConfig>) -> Self {
        Self::new(
            EthereumDataSource::new_from_parts(provider.clone(), blobs.clone(), &cfg),
            provider,
            blobs,
            cfg,
        )
    }

    /// Prefetches all data-availability items for `block_number`.
    pub async fn prefetch_block_by_number(
        mut chain_provider: C,
        blob_provider: B,
        rollup_config: Arc<RollupConfig>,
        block_number: u64,
        batcher_address: Address,
    ) -> PrefetchResult {
        let block_ref =
            chain_provider.block_info_by_number(block_number).await.map_err(Into::into)?;

        let mut source = EthereumDataSource::new_from_parts(
            chain_provider,
            blob_provider,
            rollup_config.as_ref(),
        );
        let mut prefetched = VecDeque::new();
        loop {
            match source.next(&block_ref, batcher_address).await {
                Ok(data) => prefetched.push_back(data),
                Err(PipelineErrorKind::Temporary(PipelineError::Eof)) => {
                    return Ok((block_ref, batcher_address, prefetched));
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Scans future L1 blocks until enough useful data is found or the scan limit is reached.
    pub async fn scan_prefetch_blocks(
        chain_provider: C,
        blob_provider: B,
        rollup_config: Arc<RollupConfig>,
        start_number: u64,
        batcher_address: Address,
        useful_target: usize,
        output: Sender<PrefetchMessage>,
    ) {
        let mut useful = 0;
        let mut next_offset = 0;
        let mut pending = FuturesUnordered::new();
        loop {
            while pending.len() < PREFETCH_SCAN_CONCURRENCY
                && next_offset < PREFETCH_SCAN_BLOCK_LIMIT
            {
                let Some(block_number) = start_number.checked_add(next_offset as u64) else {
                    return;
                };
                next_offset += 1;
                pending.push(Self::prefetch_block_by_number(
                    chain_provider.clone(),
                    blob_provider.clone(),
                    Arc::clone(&rollup_config),
                    block_number,
                    batcher_address,
                ));
            }

            let Some(result) = pending.next().await else {
                return;
            };
            let result_is_useful = result.as_ref().is_ok_and(|(_, _, data)| !data.is_empty());
            if output.send(result).is_err() {
                return;
            }
            if result_is_useful {
                useful += 1;
                if useful >= useful_target {
                    return;
                }
            }
        }
    }

    /// Polls streamed prefetch worker results into the local caches.
    pub async fn collect_prefetch_updates(&mut self) {
        self.drain_prefetch_messages();
        let Some((_, _, _, worker)) = self.prefetch_worker.as_ref() else {
            return;
        };
        if !worker.is_finished() {
            return;
        }

        let Some((_, _, _, worker)) = self.prefetch_worker.take() else {
            return;
        };
        if let Err(error) = worker.await {
            self.record_prefetch_join_error(error);
        }
        self.drain_prefetch_messages();
        self.prefetch_rx = None;
        self.record_inflight_len();
    }

    /// Drains streamed prefetch worker messages without blocking the pipeline.
    pub fn drain_prefetch_messages(&mut self) {
        let Some(rx) = self.prefetch_rx.as_ref().cloned() else {
            return;
        };
        let mut messages = Vec::new();
        let rx = rx.lock().expect("prefetch receiver lock poisoned");
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }
        for message in messages {
            self.store_prefetch_message(message);
        }
    }

    /// Clears stale lookahead if the retrieval cursor moved backwards or changed batcher.
    pub fn handle_request_cursor(&mut self, block_ref: &BlockInfo, batcher_address: Address) {
        let stale_cursor = self.last_request.is_some_and(|(last_number, last_batcher)| {
            last_batcher != batcher_address || block_ref.number < last_number
        });
        self.last_request = Some((block_ref.number, batcher_address));
        if !stale_cursor {
            return;
        }

        let dropped = self.prefetched.len() + self.empty_prefetched.len();
        self.prefetched.clear();
        self.empty_prefetched.clear();
        if dropped > 0 {
            Metrics::l1_prefetch_outcomes("stale").increment(dropped as u64);
        }
        if let Some((_, _, _, worker)) = self.prefetch_worker.take() {
            worker.abort();
            Metrics::l1_prefetch_outcomes("aborted").increment(1);
        }
        self.prefetch_rx = None;
        self.record_buffer_len();
        self.record_empty_len();
        self.record_inflight_len();
    }

    /// Starts a background worker that crawls ahead until it fills the useful buffer.
    pub fn start_prefetch_worker(&mut self, block_ref: &BlockInfo, batcher_address: Address) {
        self.drop_stale_prefetch_worker(block_ref, batcher_address);

        if self.prefetch_worker.is_some() || self.prefetched.len() >= PREFETCH_BUFFER_CAPACITY {
            return;
        }

        let Some(start_number) = self.next_prefetch_start_number(block_ref, batcher_address) else {
            return;
        };
        let useful_target = PREFETCH_BUFFER_CAPACITY - self.prefetched.len();
        let end_number =
            start_number.saturating_add(PREFETCH_SCAN_BLOCK_LIMIT.saturating_sub(1) as u64);
        let (tx, rx) = mpsc::channel();
        let chain_provider = self.prefetch_chain_provider.clone();
        let blob_provider = self.prefetch_blob_provider.clone();
        let rollup_config = Arc::clone(&self.rollup_config);
        let worker = tokio::spawn(async move {
            Self::scan_prefetch_blocks(
                chain_provider,
                blob_provider,
                rollup_config,
                start_number,
                batcher_address,
                useful_target,
                tx,
            )
            .await;
        });
        self.prefetch_rx = Some(Arc::new(Mutex::new(rx)));
        self.prefetch_worker = Some((start_number, end_number, batcher_address, worker));
        self.record_inflight_len();
    }

    /// Returns the next L1 block number the prefetch worker should scan.
    pub fn next_prefetch_start_number(
        &self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> Option<u64> {
        let mut start = block_ref.number.checked_add(1)?;
        for (block, batcher, _) in &self.prefetched {
            if *batcher == batcher_address && block.number >= start {
                start = block.number.checked_add(1)?;
            }
        }
        for (block, batcher) in &self.empty_prefetched {
            if *batcher == batcher_address && block.number >= start {
                start = block.number.checked_add(1)?;
            }
        }
        Some(start)
    }

    /// Drops a stale prefetch worker that no longer covers future blocks for the current request.
    pub fn drop_stale_prefetch_worker(&mut self, block_ref: &BlockInfo, batcher_address: Address) {
        let stale = self.prefetch_worker.as_ref().is_some_and(|(_, end_number, batcher, _)| {
            *batcher != batcher_address || *end_number <= block_ref.number
        });
        if stale {
            if let Some((_, _, _, worker)) = self.prefetch_worker.take() {
                worker.abort();
                Metrics::l1_prefetch_outcomes("aborted").increment(1);
            }
            self.prefetch_rx = None;
            self.record_inflight_len();
        }
    }

    /// Returns whether the completed prefetch cache matches the requested block.
    pub fn prefetched_matches(&self, block_ref: &BlockInfo, batcher_address: Address) -> bool {
        self.prefetched.front().is_some_and(|(block, batcher, _)| {
            block.hash == block_ref.hash && *batcher == batcher_address
        })
    }

    /// Moves a matching cached prefetch to the front of the ring buffer.
    pub fn promote_matching_prefetch(&mut self, block_ref: &BlockInfo, batcher_address: Address) {
        if self.prefetched_matches(block_ref, batcher_address) {
            return;
        }
        let Some(index) = self.prefetched.iter().position(|(block, batcher, _)| {
            block.hash == block_ref.hash && *batcher == batcher_address
        }) else {
            return;
        };
        let Some(result) = self.prefetched.remove(index) else {
            return;
        };
        self.prefetched.push_front(result);
    }

    /// Serves one item from the completed useful prefetch cache.
    pub fn pop_prefetched(&mut self) -> Option<Bytes> {
        let (_, _, data) = self.prefetched.front_mut()?;
        match data.pop_front() {
            Some(data) => Some(data),
            None => {
                self.prefetched.pop_front();
                None
            }
        }
    }

    /// Removes a matching empty prefetch if one is cached.
    pub fn take_empty_prefetch(&mut self, block_ref: &BlockInfo, batcher_address: Address) -> bool {
        let Some(index) = self.empty_prefetched.iter().position(|(block, batcher)| {
            block.hash == block_ref.hash && *batcher == batcher_address
        }) else {
            return false;
        };
        self.empty_prefetched.remove(index);
        self.record_empty_len();
        true
    }

    /// Records a prefetch task error.
    pub fn record_prefetch_error(&self, error: PipelineErrorKind) {
        Metrics::l1_prefetch_outcomes("error").increment(1);
        debug!(target: "l1_prefetch", error = %error, "L1 data prefetch failed");
    }

    /// Records a prefetch task join error.
    pub fn record_prefetch_join_error(&self, error: tokio::task::JoinError) {
        Metrics::l1_prefetch_outcomes("error").increment(1);
        debug!(target: "l1_prefetch", error = %error, "L1 data prefetch task failed");
    }

    /// Stores a streamed prefetch message or records the failure.
    pub fn store_prefetch_message(&mut self, message: PrefetchMessage) {
        match message {
            Ok(result) => self.store_prefetch_result(result),
            Err(error) => self.record_prefetch_error(error),
        }
    }

    /// Records the number of completed prefetch results available to the pipeline.
    pub fn record_buffer_len(&self) {
        Metrics::l1_prefetch_buffer_len().set(self.prefetched.len() as f64);
    }

    /// Records the number of empty prefetch metadata entries available to the pipeline.
    pub fn record_empty_len(&self) {
        Metrics::l1_prefetch_empty_len().set(self.empty_prefetched.len() as f64);
    }

    /// Records whether a prefetch worker is in flight.
    pub fn record_inflight_len(&self) {
        let in_flight = f64::from(self.prefetch_worker.is_some());
        Metrics::l1_prefetch_inflight_len().set(in_flight);
    }

    /// Stores a completed useful prefetch result without overwriting a block that is still draining.
    pub fn store_prefetch_result(&mut self, result: PrefetchedData) {
        let (result_block, result_batcher, _) = &result;
        if result.2.is_empty() {
            self.store_empty_prefetch((*result_block, *result_batcher));
            return;
        }
        let result_key = (result_block.number, result_block.hash, *result_batcher);
        if let Some(index) = self.prefetched.iter().position(|(block, batcher, _)| {
            block.number == result_block.number
                && block.hash == result_block.hash
                && *batcher == *result_batcher
        }) {
            self.prefetched.remove(index);
        }

        let insertion_index = self
            .prefetched
            .iter()
            .position(|(block, _, _)| block.number > result_block.number)
            .unwrap_or(self.prefetched.len());
        self.prefetched.insert(insertion_index, result);

        let mut stored = true;
        if self.prefetched.len() > PREFETCH_BUFFER_CAPACITY
            && let Some((block, batcher, _)) = self.prefetched.pop_back()
        {
            stored = (block.number, block.hash, batcher) != result_key;
            Metrics::l1_prefetch_outcomes("evicted").increment(1);
        }
        if stored {
            Metrics::l1_prefetch_outcomes("stored").increment(1);
        }
        self.record_buffer_len();
    }

    /// Stores completed empty prefetch metadata for fast EOF.
    pub fn store_empty_prefetch(&mut self, result: EmptyPrefetch) {
        let (result_block, result_batcher) = result;
        if self.empty_prefetched.iter().any(|(block, batcher)| {
            block.number == result_block.number
                && block.hash == result_block.hash
                && *batcher == result_batcher
        }) {
            return;
        }

        let insertion_index = self
            .empty_prefetched
            .iter()
            .position(|(block, _)| block.number > result_block.number)
            .unwrap_or(self.empty_prefetched.len());
        self.empty_prefetched.insert(insertion_index, result);

        if self.empty_prefetched.len() > PREFETCH_EMPTY_BUFFER_CAPACITY {
            self.empty_prefetched.pop_back();
            Metrics::l1_prefetch_outcomes("evicted").increment(1);
        }
        Metrics::l1_prefetch_outcomes("empty").increment(1);
        self.record_empty_len();
    }

    /// Drops completed useful and empty prefetches that are stale for the current request.
    pub fn drop_stale_prefetches(&mut self, block_ref: &BlockInfo, batcher_address: Address) {
        let before_prefetched = self.prefetched.len();
        self.prefetched
            .retain(|prefetched| !Self::prefetch_is_stale(prefetched, block_ref, batcher_address));
        let dropped_prefetched = before_prefetched - self.prefetched.len();
        if dropped_prefetched > 0 {
            Metrics::l1_prefetch_outcomes("stale").increment(dropped_prefetched as u64);
            self.record_buffer_len();
        }

        let before_empty = self.empty_prefetched.len();
        self.empty_prefetched.retain(|prefetched| {
            !Self::empty_prefetch_is_stale(prefetched, block_ref, batcher_address)
        });
        let dropped_empty = before_empty - self.empty_prefetched.len();
        if dropped_empty > 0 {
            Metrics::l1_prefetch_outcomes("stale").increment(dropped_empty as u64);
            self.record_empty_len();
        }
    }

    /// Returns whether a completed useful prefetch is stale relative to the current request.
    pub fn prefetch_is_stale(
        prefetched: &PrefetchedData,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> bool {
        let (block, batcher, _) = prefetched;
        if block.hash == block_ref.hash && *batcher == batcher_address {
            return false;
        }
        *batcher != batcher_address || block.number <= block_ref.number
    }

    /// Returns whether completed empty prefetch metadata is stale relative to the current request.
    pub fn empty_prefetch_is_stale(
        prefetched: &EmptyPrefetch,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> bool {
        let (block, batcher) = prefetched;
        if block.hash == block_ref.hash && *batcher == batcher_address {
            return false;
        }
        *batcher != batcher_address || block.number <= block_ref.number
    }
}

#[async_trait]
impl<C, B> DataAvailabilityProvider for PrefetchingEthereumDataSource<C, B>
where
    C: ChainProvider + Send + Sync + Clone + Debug + 'static,
    B: BlobProvider + Send + Sync + Clone + Debug + 'static,
{
    type Item = Bytes;

    async fn next(
        &mut self,
        block_ref: &BlockInfo,
        batcher_address: Address,
    ) -> PipelineResult<Self::Item> {
        self.handle_request_cursor(block_ref, batcher_address);
        self.collect_prefetch_updates().await;
        self.promote_matching_prefetch(block_ref, batcher_address);
        if self.prefetched_matches(block_ref, batcher_address) {
            Metrics::l1_prefetch_outcomes("hit").increment(1);
            self.start_prefetch_worker(block_ref, batcher_address);
            let result = self.pop_prefetched().ok_or(PipelineError::Eof.temp());
            self.record_buffer_len();
            return result;
        }
        if self.take_empty_prefetch(block_ref, batcher_address) {
            Metrics::l1_prefetch_outcomes("empty_hit").increment(1);
            self.start_prefetch_worker(block_ref, batcher_address);
            return Err(PipelineError::Eof.temp());
        }

        self.drop_stale_prefetches(block_ref, batcher_address);

        Metrics::l1_prefetch_outcomes("miss").increment(1);
        self.start_prefetch_worker(block_ref, batcher_address);
        self.source.next(block_ref, batcher_address).await
    }

    fn clear(&mut self) {
        self.source.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fmt::{Display, Formatter},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        vec,
    };

    use alloy_consensus::{Header, Receipt, TxEnvelope};
    use alloy_eips::eip4844::Blob;
    use alloy_primitives::{B256, b256};
    use tokio::task::yield_now;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestProviderError;

    impl Display for TestProviderError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str("test provider error")
        }
    }

    impl From<TestProviderError> for PipelineErrorKind {
        fn from(_: TestProviderError) -> Self {
            PipelineError::Provider("test provider error".to_string()).temp()
        }
    }

    #[derive(Debug, Clone, Default)]
    struct TestCounters {
        block_by_number: Arc<AtomicUsize>,
        block_by_hash: Arc<AtomicUsize>,
    }

    #[derive(Debug, Clone, Default)]
    struct TestChainProvider {
        blocks_by_number: Arc<HashMap<u64, BlockInfo>>,
        blocks_by_hash: Arc<HashMap<B256, (BlockInfo, Vec<TxEnvelope>)>>,
        counters: TestCounters,
    }

    impl TestChainProvider {
        fn new(blocks: Vec<BlockInfo>) -> Self {
            let blocks_by_number = blocks.iter().map(|block| (block.number, *block)).collect();
            let blocks_by_hash =
                blocks.into_iter().map(|block| (block.hash, (block, Vec::new()))).collect();
            Self {
                blocks_by_number: Arc::new(blocks_by_number),
                blocks_by_hash: Arc::new(blocks_by_hash),
                counters: TestCounters::default(),
            }
        }
    }

    #[async_trait]
    impl ChainProvider for TestChainProvider {
        type Error = TestProviderError;

        async fn header_by_hash(&mut self, _: B256) -> Result<Header, Self::Error> {
            Err(TestProviderError)
        }

        async fn block_info_by_number(&mut self, number: u64) -> Result<BlockInfo, Self::Error> {
            self.counters.block_by_number.fetch_add(1, Ordering::Relaxed);
            self.blocks_by_number.get(&number).copied().ok_or(TestProviderError)
        }

        async fn receipts_by_hash(&mut self, _: B256) -> Result<Vec<Receipt>, Self::Error> {
            Err(TestProviderError)
        }

        async fn block_info_and_transactions_by_hash(
            &mut self,
            hash: B256,
        ) -> Result<(BlockInfo, Vec<TxEnvelope>), Self::Error> {
            self.counters.block_by_hash.fetch_add(1, Ordering::Relaxed);
            self.blocks_by_hash.get(&hash).cloned().ok_or(TestProviderError)
        }
    }

    #[derive(Debug, Clone, Default)]
    struct TestBlobProvider;

    #[async_trait]
    impl BlobProvider for TestBlobProvider {
        type Error = TestProviderError;

        async fn get_and_validate_blobs(
            &mut self,
            _: &BlockInfo,
            _: &[B256],
        ) -> Result<Vec<Box<Blob>>, Self::Error> {
            Ok(Vec::new())
        }
    }

    fn test_block(number: u64) -> BlockInfo {
        BlockInfo {
            number,
            hash: B256::with_last_byte(number as u8),
            parent_hash: B256::ZERO,
            timestamp: number,
        }
    }

    #[test]
    fn empty_prefetch_overflow_retains_nearest_future_entries() {
        let cfg = Arc::new(RollupConfig::default());
        let active_source = EthereumDataSource::new_from_parts(
            TestChainProvider::default(),
            TestBlobProvider,
            cfg.as_ref(),
        );
        let mut source = PrefetchingEthereumDataSource::new(
            active_source,
            TestChainProvider::default(),
            TestBlobProvider,
            cfg,
        );

        for number in 1..=(PREFETCH_EMPTY_BUFFER_CAPACITY as u64 + 10) {
            source.store_empty_prefetch((test_block(number), Address::ZERO));
        }

        assert_eq!(source.empty_prefetched.len(), PREFETCH_EMPTY_BUFFER_CAPACITY);
        assert_eq!(source.empty_prefetched.front().unwrap().0.number, 1);
        assert_eq!(
            source.empty_prefetched.back().unwrap().0.number,
            PREFETCH_EMPTY_BUFFER_CAPACITY as u64
        );
        assert!(
            source
                .empty_prefetched
                .iter()
                .all(|(block, _)| block.number <= PREFETCH_EMPTY_BUFFER_CAPACITY as u64)
        );
    }

    #[test]
    fn cursor_rewind_drops_stale_data_availability_prefetches() {
        let cfg = Arc::new(RollupConfig::default());
        let active_source = EthereumDataSource::new_from_parts(
            TestChainProvider::default(),
            TestBlobProvider,
            cfg.as_ref(),
        );
        let mut source = PrefetchingEthereumDataSource::new(
            active_source,
            TestChainProvider::default(),
            TestBlobProvider,
            cfg,
        );
        source.prefetched.push_back((
            test_block(100),
            Address::ZERO,
            VecDeque::from([Bytes::from_static(b"data")]),
        ));
        source.empty_prefetched.push_back((test_block(101), Address::ZERO));
        source.last_request = Some((101, Address::ZERO));

        source.handle_request_cursor(&test_block(50), Address::ZERO);

        assert!(source.prefetched.is_empty());
        assert!(source.empty_prefetched.is_empty());
        assert_eq!(source.last_request, Some((50, Address::ZERO)));
    }

    #[tokio::test]
    async fn serves_matching_empty_prefetch_without_live_l1_hash_fetch() {
        let block_1 = BlockInfo {
            number: 1,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000001"),
            parent_hash: B256::ZERO,
            timestamp: 1,
        };
        let block_2 = BlockInfo {
            number: 2,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000002"),
            parent_hash: block_1.hash,
            timestamp: 2,
        };
        let block_3 = BlockInfo {
            number: 3,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000003"),
            parent_hash: block_2.hash,
            timestamp: 3,
        };
        let block_4 = BlockInfo {
            number: 4,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000004"),
            parent_hash: block_3.hash,
            timestamp: 4,
        };
        let block_5 = BlockInfo {
            number: 5,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000005"),
            parent_hash: block_4.hash,
            timestamp: 5,
        };
        let block_6 = BlockInfo {
            number: 6,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000006"),
            parent_hash: block_5.hash,
            timestamp: 6,
        };
        let block_7 = BlockInfo {
            number: 7,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000007"),
            parent_hash: block_6.hash,
            timestamp: 7,
        };
        let block_8 = BlockInfo {
            number: 8,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000008"),
            parent_hash: block_7.hash,
            timestamp: 8,
        };
        let block_9 = BlockInfo {
            number: 9,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000009"),
            parent_hash: block_8.hash,
            timestamp: 9,
        };
        let block_10 = BlockInfo {
            number: 10,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000010"),
            parent_hash: block_9.hash,
            timestamp: 10,
        };
        let blocks = vec![
            block_1, block_2, block_3, block_4, block_5, block_6, block_7, block_8, block_9,
            block_10,
        ];
        let active_chain = TestChainProvider::new(blocks.clone());
        let prefetch_chain = TestChainProvider::new(blocks);
        let active_counters = active_chain.counters.clone();
        let prefetch_counters = prefetch_chain.counters.clone();
        let cfg = Arc::new(RollupConfig::default());
        let active_source =
            EthereumDataSource::new_from_parts(active_chain, TestBlobProvider, cfg.as_ref());
        let mut source = PrefetchingEthereumDataSource::new(
            active_source,
            prefetch_chain,
            TestBlobProvider,
            cfg,
        );

        assert!(matches!(
            source.next(&block_1, Address::ZERO).await,
            Err(PipelineErrorKind::Temporary(PipelineError::Eof))
        ));
        for _ in 0..32 {
            yield_now().await;
            source.collect_prefetch_updates().await;
            if source.prefetch_worker.is_none() {
                break;
            }
        }
        assert!(source.prefetched.is_empty());
        assert!(source.empty_prefetched.len() >= PREFETCH_BUFFER_CAPACITY);

        source.clear();
        assert!(matches!(
            source.next(&block_2, Address::ZERO).await,
            Err(PipelineErrorKind::Temporary(PipelineError::Eof))
        ));

        assert_eq!(active_counters.block_by_hash.load(Ordering::Relaxed), 1);
        assert!(prefetch_counters.block_by_number.load(Ordering::Relaxed) >= 10);
        assert_eq!(prefetch_counters.block_by_hash.load(Ordering::Relaxed), 9);
    }

    #[tokio::test]
    async fn keeps_next_lookahead_while_draining_current_prefetch() {
        let block_1 = BlockInfo {
            number: 1,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000001"),
            parent_hash: B256::ZERO,
            timestamp: 1,
        };
        let block_2 = BlockInfo {
            number: 2,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000002"),
            parent_hash: block_1.hash,
            timestamp: 2,
        };
        let block_3 = BlockInfo {
            number: 3,
            hash: b256!("0000000000000000000000000000000000000000000000000000000000000003"),
            parent_hash: block_2.hash,
            timestamp: 3,
        };
        let active_chain = TestChainProvider::new(vec![block_1, block_2, block_3]);
        let prefetch_chain = TestChainProvider::new(vec![block_1, block_2, block_3]);
        let active_source = EthereumDataSource::new_from_parts(
            active_chain,
            TestBlobProvider,
            &RollupConfig::default(),
        );
        let mut source = PrefetchingEthereumDataSource::new(
            active_source,
            prefetch_chain,
            TestBlobProvider,
            Arc::new(RollupConfig::default()),
        );
        source.prefetched.push_back((
            block_2,
            Address::ZERO,
            VecDeque::from([Bytes::from_static(b"first"), Bytes::from_static(b"second")]),
        ));
        source.prefetched.push_back((
            block_3,
            Address::ZERO,
            VecDeque::from([Bytes::from_static(b"third")]),
        ));

        assert_eq!(
            source.next(&block_2, Address::ZERO).await.unwrap(),
            Bytes::from_static(b"first")
        );
        assert_eq!(
            source.next(&block_2, Address::ZERO).await.unwrap(),
            Bytes::from_static(b"second")
        );
        assert!(matches!(
            source.next(&block_2, Address::ZERO).await,
            Err(PipelineErrorKind::Temporary(PipelineError::Eof))
        ));
        assert_eq!(
            source.next(&block_3, Address::ZERO).await.unwrap(),
            Bytes::from_static(b"third")
        );
    }
}
