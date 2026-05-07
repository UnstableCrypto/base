//! Online L1 origin prefetching for derivation traversal.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
};

use alloy_consensus::{Header, Receipt, TxEnvelope};
use alloy_primitives::B256;
use async_trait::async_trait;
use base_consensus_derive::ChainProvider;
use base_protocol::BlockInfo;
use futures::{StreamExt, stream::FuturesUnordered};
use tokio::task::JoinHandle;
use tracing::debug;

use crate::Metrics;

/// The maximum number of completed L1 origin prefetch results retained locally.
pub const ORIGIN_PREFETCH_BUFFER_CAPACITY: usize = 2048;

/// The maximum number of L1 origins a worker scans while filling the buffer.
pub const ORIGIN_PREFETCH_SCAN_BLOCK_LIMIT: usize = 2048;

/// The maximum number of concurrent L1 origin fetches in one prefetch worker.
pub const ORIGIN_PREFETCH_SCAN_CONCURRENCY: usize = 32;

/// Completed origin traversal data for one L1 block.
pub type OriginPrefetchData = (BlockInfo, Vec<Receipt>);

/// The data collected by an L1 origin prefetch task.
pub type OriginPrefetchResult<E> = Result<OriginPrefetchData, E>;

/// A streamed result from the L1 origin prefetch worker.
pub type OriginPrefetchMessage<E> = OriginPrefetchResult<E>;

/// Receiver for streamed L1 origin prefetch worker results.
pub type OriginPrefetchReceiver<E> = Arc<Mutex<Receiver<OriginPrefetchMessage<E>>>>;

/// An active L1 origin prefetch worker: first block, last block, task.
pub type OriginPrefetchWorker = (u64, u64, JoinHandle<()>);

/// A bounded lookahead wrapper for origin traversal block refs and receipts.
#[derive(Debug)]
pub struct PrefetchingChainProvider<C>
where
    C: ChainProvider,
{
    /// The synchronous fallback provider used for requested data that is not prefetched.
    pub inner: C,
    /// Completed prefetched origin data, ordered by L1 block number.
    pub prefetched: VecDeque<OriginPrefetchData>,
    /// Receiver for streamed prefetch worker results.
    pub prefetch_rx: Option<OriginPrefetchReceiver<C::Error>>,
    /// Active prefetch worker metadata.
    pub prefetch_worker: Option<OriginPrefetchWorker>,
    /// Last L1 block number requested by the pipeline.
    pub last_requested_number: Option<u64>,
}

impl<C> Clone for PrefetchingChainProvider<C>
where
    C: ChainProvider + Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            prefetched: VecDeque::new(),
            prefetch_rx: None,
            prefetch_worker: None,
            last_requested_number: None,
        }
    }
}

impl<C> PrefetchingChainProvider<C>
where
    C: ChainProvider + Send + Clone + Debug + 'static,
    C::Error: Send + 'static,
{
    /// Creates a new prefetching chain provider.
    pub const fn new(inner: C) -> Self {
        Self {
            inner,
            prefetched: VecDeque::new(),
            prefetch_rx: None,
            prefetch_worker: None,
            last_requested_number: None,
        }
    }

    /// Prefetches the block ref and receipts for `block_number`.
    pub async fn prefetch_origin_by_number(
        mut provider: C,
        block_number: u64,
    ) -> OriginPrefetchResult<C::Error> {
        let block_ref = provider.block_info_by_number(block_number).await?;
        let receipts = provider.receipts_by_hash(block_ref.hash).await?;
        Ok((block_ref, receipts))
    }

    /// Scans future L1 origins until enough results are found or the scan limit is reached.
    pub async fn scan_prefetch_origins(
        provider: C,
        start_number: u64,
        target: usize,
        output: Sender<OriginPrefetchMessage<C::Error>>,
    ) {
        let mut stored = 0;
        let mut next_offset = 0;
        let mut pending = FuturesUnordered::new();
        loop {
            while pending.len() < ORIGIN_PREFETCH_SCAN_CONCURRENCY
                && next_offset < ORIGIN_PREFETCH_SCAN_BLOCK_LIMIT
            {
                let Some(block_number) = start_number.checked_add(next_offset as u64) else {
                    return;
                };
                next_offset += 1;
                pending.push(Self::prefetch_origin_by_number(provider.clone(), block_number));
            }

            let Some(result) = pending.next().await else {
                return;
            };
            let result_is_ok = result.is_ok();
            if output.send(result).is_err() {
                return;
            }
            if !result_is_ok {
                return;
            }
            stored += 1;
            if stored >= target {
                return;
            }
        }
    }

    /// Polls streamed prefetch worker results into the local cache.
    pub async fn collect_prefetch_updates(&mut self) {
        self.drain_prefetch_messages();
        let Some((_, _, worker)) = self.prefetch_worker.as_ref() else {
            return;
        };
        if !worker.is_finished() {
            return;
        }

        let Some((_, _, worker)) = self.prefetch_worker.take() else {
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
        let rx = rx.lock().expect("origin prefetch receiver lock poisoned");
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }
        for message in messages {
            self.store_prefetch_message(message);
        }
    }

    /// Clears stale lookahead if the traversal cursor moved backwards.
    pub fn handle_requested_number(&mut self, block_number: u64) {
        let rewound =
            self.last_requested_number.is_some_and(|last_requested| block_number < last_requested);
        self.last_requested_number = Some(block_number);
        if !rewound {
            return;
        }

        let dropped = self.prefetched.len();
        self.prefetched.clear();
        if dropped > 0 {
            Metrics::l1_origin_prefetch_outcomes("stale").increment(dropped as u64);
        }
        if let Some((_, _, worker)) = self.prefetch_worker.take() {
            worker.abort();
            Metrics::l1_origin_prefetch_outcomes("aborted").increment(1);
        }
        self.prefetch_rx = None;
        self.record_buffer_len();
        self.record_inflight_len();
    }

    /// Starts a background worker that fills the origin traversal buffer.
    pub fn start_prefetch_worker(&mut self, block_number: u64) {
        self.drop_stale_prefetch_worker(block_number);

        if self.prefetch_worker.is_some()
            || self.prefetched.len() >= ORIGIN_PREFETCH_BUFFER_CAPACITY
        {
            return;
        }

        let Some(start_number) = self.next_prefetch_start_number(block_number) else {
            return;
        };
        let target = ORIGIN_PREFETCH_BUFFER_CAPACITY - self.prefetched.len();
        let end_number =
            start_number.saturating_add(ORIGIN_PREFETCH_SCAN_BLOCK_LIMIT.saturating_sub(1) as u64);
        let (tx, rx) = mpsc::channel();
        let provider = self.inner.clone();
        let worker = tokio::spawn(async move {
            Self::scan_prefetch_origins(provider, start_number, target, tx).await;
        });
        self.prefetch_rx = Some(Arc::new(Mutex::new(rx)));
        self.prefetch_worker = Some((start_number, end_number, worker));
        self.record_inflight_len();
    }

    /// Returns the next L1 block number the prefetch worker should scan.
    pub fn next_prefetch_start_number(&self, block_number: u64) -> Option<u64> {
        let mut start = block_number.checked_add(1)?;
        for (block, _) in &self.prefetched {
            if block.number >= start {
                start = block.number.checked_add(1)?;
            }
        }
        Some(start)
    }

    /// Drops a stale prefetch worker that no longer covers future blocks for the current request.
    pub fn drop_stale_prefetch_worker(&mut self, block_number: u64) {
        let stale = self
            .prefetch_worker
            .as_ref()
            .is_some_and(|(_, end_number, _)| *end_number <= block_number);
        if stale {
            if let Some((_, _, worker)) = self.prefetch_worker.take() {
                worker.abort();
                Metrics::l1_origin_prefetch_outcomes("aborted").increment(1);
            }
            self.prefetch_rx = None;
            self.record_inflight_len();
        }
    }

    /// Returns a matching prefetched block ref without consuming the matching receipts.
    pub fn prefetched_block(&self, block_number: u64) -> Option<BlockInfo> {
        self.prefetched
            .iter()
            .find_map(|(block, _)| (block.number == block_number).then_some(*block))
    }

    /// Removes and returns matching prefetched receipts.
    pub fn take_prefetched_receipts(&mut self, hash: B256) -> Option<Vec<Receipt>> {
        let index = self.prefetched.iter().position(|(block, _)| block.hash == hash)?;
        let (_, receipts) = self.prefetched.remove(index)?;
        self.record_buffer_len();
        Some(receipts)
    }

    /// Records a prefetch task error.
    pub fn record_prefetch_error(&self, error: C::Error) {
        Metrics::l1_origin_prefetch_outcomes("error").increment(1);
        debug!(target: "l1_origin_prefetch", error = %error, "L1 origin prefetch failed");
    }

    /// Records a prefetch task join error.
    pub fn record_prefetch_join_error(&self, error: tokio::task::JoinError) {
        Metrics::l1_origin_prefetch_outcomes("error").increment(1);
        debug!(target: "l1_origin_prefetch", error = %error, "L1 origin prefetch task failed");
    }

    /// Stores a streamed prefetch message or records the failure.
    pub fn store_prefetch_message(&mut self, message: OriginPrefetchMessage<C::Error>) {
        match message {
            Ok(result) => self.store_prefetch_result(result),
            Err(error) => self.record_prefetch_error(error),
        }
    }

    /// Records the number of completed origin prefetch results available to the pipeline.
    pub fn record_buffer_len(&self) {
        Metrics::l1_origin_prefetch_buffer_len().set(self.prefetched.len() as f64);
    }

    /// Records whether an origin prefetch worker is in flight.
    pub fn record_inflight_len(&self) {
        let in_flight = f64::from(self.prefetch_worker.is_some());
        Metrics::l1_origin_prefetch_inflight_len().set(in_flight);
    }

    /// Stores a completed origin prefetch result.
    pub fn store_prefetch_result(&mut self, result: OriginPrefetchData) {
        let result_block = result.0;
        if let Some(index) = self.prefetched.iter().position(|(block, _)| {
            block.number == result_block.number && block.hash == result_block.hash
        }) {
            self.prefetched.remove(index);
        }

        let insertion_index = self
            .prefetched
            .iter()
            .position(|(block, _)| block.number > result_block.number)
            .unwrap_or(self.prefetched.len());
        self.prefetched.insert(insertion_index, result);

        let mut stored = true;
        if self.prefetched.len() > ORIGIN_PREFETCH_BUFFER_CAPACITY
            && let Some((block, _)) = self.prefetched.pop_back()
        {
            stored = block.number != result_block.number || block.hash != result_block.hash;
            Metrics::l1_origin_prefetch_outcomes("evicted").increment(1);
        }
        if stored {
            Metrics::l1_origin_prefetch_outcomes("stored").increment(1);
        }
        self.record_buffer_len();
    }

    /// Drops completed origin prefetches that are stale for the current request.
    pub fn drop_stale_prefetches(&mut self, block_number: u64) {
        let before = self.prefetched.len();
        self.prefetched.retain(|(block, _)| block.number >= block_number);
        let dropped = before - self.prefetched.len();
        if dropped > 0 {
            Metrics::l1_origin_prefetch_outcomes("stale").increment(dropped as u64);
            self.record_buffer_len();
        }
    }
}

#[async_trait]
impl<C> ChainProvider for PrefetchingChainProvider<C>
where
    C: ChainProvider + Send + Sync + Clone + Debug + 'static,
    C::Error: Send + 'static,
{
    type Error = C::Error;

    async fn header_by_hash(&mut self, hash: B256) -> Result<Header, Self::Error> {
        self.inner.header_by_hash(hash).await
    }

    async fn block_info_by_number(&mut self, number: u64) -> Result<BlockInfo, Self::Error> {
        self.handle_requested_number(number);
        self.collect_prefetch_updates().await;
        self.drop_stale_prefetches(number);
        if let Some(block) = self.prefetched_block(number) {
            Metrics::l1_origin_prefetch_outcomes("block_hit").increment(1);
            self.start_prefetch_worker(number);
            return Ok(block);
        }

        Metrics::l1_origin_prefetch_outcomes("miss").increment(1);
        self.start_prefetch_worker(number);
        self.inner.block_info_by_number(number).await
    }

    async fn receipts_by_hash(&mut self, hash: B256) -> Result<Vec<Receipt>, Self::Error> {
        self.collect_prefetch_updates().await;
        if let Some(receipts) = self.take_prefetched_receipts(hash) {
            Metrics::l1_origin_prefetch_outcomes("receipts_hit").increment(1);
            return Ok(receipts);
        }

        self.inner.receipts_by_hash(hash).await
    }

    async fn block_info_and_transactions_by_hash(
        &mut self,
        hash: B256,
    ) -> Result<(BlockInfo, Vec<TxEnvelope>), Self::Error> {
        self.inner.block_info_and_transactions_by_hash(hash).await
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

    use alloy_primitives::b256;
    use base_consensus_derive::{PipelineError, PipelineErrorKind};
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
        receipts_by_hash: Arc<AtomicUsize>,
    }

    #[derive(Debug, Clone, Default)]
    struct TestChainProvider {
        blocks_by_number: Arc<HashMap<u64, BlockInfo>>,
        receipts_by_hash: Arc<HashMap<B256, Vec<Receipt>>>,
        counters: TestCounters,
    }

    impl TestChainProvider {
        fn new(blocks: Vec<BlockInfo>) -> Self {
            let blocks_by_number = blocks.iter().map(|block| (block.number, *block)).collect();
            let receipts_by_hash =
                blocks.into_iter().map(|block| (block.hash, Vec::new())).collect();
            Self {
                blocks_by_number: Arc::new(blocks_by_number),
                receipts_by_hash: Arc::new(receipts_by_hash),
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

        async fn receipts_by_hash(&mut self, hash: B256) -> Result<Vec<Receipt>, Self::Error> {
            self.counters.receipts_by_hash.fetch_add(1, Ordering::Relaxed);
            self.receipts_by_hash.get(&hash).cloned().ok_or(TestProviderError)
        }

        async fn block_info_and_transactions_by_hash(
            &mut self,
            _: B256,
        ) -> Result<(BlockInfo, Vec<TxEnvelope>), Self::Error> {
            Err(TestProviderError)
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
    fn stores_prefetched_origins_in_number_order() {
        let mut provider = PrefetchingChainProvider::new(TestChainProvider::default());
        let block_1 = test_block(1);
        let block_2 = test_block(2);
        provider.store_prefetch_result((block_2, Vec::new()));
        provider.store_prefetch_result((block_1, Vec::new()));

        assert_eq!(provider.prefetched[0].0, block_1);
        assert_eq!(provider.prefetched[1].0, block_2);
    }

    #[test]
    fn cursor_rewind_drops_stale_origin_prefetches() {
        let mut provider = PrefetchingChainProvider::new(TestChainProvider::default());
        provider.store_prefetch_result((test_block(100), Vec::new()));
        provider.last_requested_number = Some(100);

        provider.handle_requested_number(50);

        assert!(provider.prefetched.is_empty());
        assert_eq!(provider.last_requested_number, Some(50));
    }

    #[tokio::test]
    async fn serves_prefetched_block_and_receipts_without_live_rpc() {
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
        let inner = TestChainProvider::new(vec![block_1, block_2, block_3]);
        let counters = inner.counters.clone();
        let mut provider = PrefetchingChainProvider::new(inner);

        assert_eq!(provider.block_info_by_number(1).await.unwrap(), block_1);
        for _ in 0..32 {
            yield_now().await;
            provider.collect_prefetch_updates().await;
            if provider.prefetched_block(2).is_some() {
                break;
            }
        }

        let block_by_number_before_hit = counters.block_by_number.load(Ordering::Relaxed);
        let receipts_by_hash_before_hit = counters.receipts_by_hash.load(Ordering::Relaxed);
        assert_eq!(provider.block_info_by_number(2).await.unwrap(), block_2);
        assert_eq!(provider.receipts_by_hash(block_2.hash).await.unwrap(), Vec::new());
        assert_eq!(counters.block_by_number.load(Ordering::Relaxed), block_by_number_before_hit);
        assert_eq!(counters.receipts_by_hash.load(Ordering::Relaxed), receipts_by_hash_before_hit);
    }
}
