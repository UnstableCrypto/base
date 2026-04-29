//! Game scanner for the challenger service.
//!
//! Scans the [`DisputeGameFactory`](base_proof_contracts::DisputeGameFactoryClient)
//! for dispute games that require action. Each game is classified into one
//! of four [`GameCategory`] variants based on its on-chain state:
//!
//! 1. **[`InvalidTeeProposal`](GameCategory::InvalidTeeProposal)** —
//!    TEE-proposed game (`teeProver != 0`, `zkProver == 0`). The driver
//!    validates the intermediate roots and, if invalid, nullifies with a
//!    TEE proof or challenges with a ZK proof.
//!
//! 2. **[`FraudulentZkChallenge`](GameCategory::FraudulentZkChallenge)** —
//!    A TEE-proposed game that has been challenged by a ZK proof
//!    (`teeProver != 0`, `zkProver != 0`, `counteredByIntermediateRootIndexPlusOne > 0`).
//!    The driver validates the originally proposed root at the challenged
//!    index and, if the original was correct, nullifies the ZK challenge
//!    with a ZK proof.
//!
//! 3. **[`InvalidZkProposal`](GameCategory::InvalidZkProposal)** —
//!    ZK-proposed game (`teeProver == 0`, `zkProver != 0`, unchallenged).
//!    The driver validates the intermediate roots and, if invalid,
//!    nullifies with a ZK proof.
//!
//! 4. **[`InvalidDualProposal`](GameCategory::InvalidDualProposal)** —
//!    Both TEE and ZK proofs are present but no challenge has been filed
//!    (`counteredByIntermediateRootIndexPlusOne == 0`). The driver
//!    nullifies the TEE proof first (fast, synchronous) and falls back to
//!    ZK nullification if TEE proving is unavailable. After the TEE proof
//!    is nullified, the subsequent scan reclassifies the game as
//!    [`InvalidZkProposal`](GameCategory::InvalidZkProposal).
//!
//! Games that are not `IN_PROGRESS` or have been fully nullified (both
//! provers zero) are skipped.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use alloy_primitives::{Address, B256};
use base_proof_contracts::{
    AggregateVerifierClient, DisputeGameFactoryClient, GameAtIndex, GameInfo, GameStatus,
};
use eyre::Result;
use futures::stream::{self, StreamExt};
use tracing::{debug, error, info, warn};

use crate::ChallengerMetrics;

/// Configuration for the game scanner.
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// Number of past games to scan on startup (lookback window).
    pub lookback_games: u64,
}

/// Classifies why a game was selected as a candidate and what action the
/// driver should take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameCategory {
    /// Path 1: TEE-proposed game with a potentially wrong output root.
    ///
    /// The driver validates the intermediate roots. If invalid it either
    /// nullifies with a TEE proof or challenges with a ZK proof.
    InvalidTeeProposal,

    /// Path 2: A TEE-proposed game was challenged with a potentially
    /// fraudulent ZK proof.
    ///
    /// The driver validates the originally proposed root at the challenged
    /// index. If the original root was actually correct, a ZK proof is
    /// submitted via `nullify()` to refute the challenge.
    FraudulentZkChallenge {
        /// The 0-based index of the challenged intermediate root.
        challenged_index: u64,
    },

    /// Path 3: ZK-proposed game with a potentially wrong output root.
    ///
    /// The driver validates the intermediate roots. If invalid it submits
    /// a ZK proof via `nullify()` to nullify the incorrect ZK proposal.
    InvalidZkProposal,

    /// Path 4: Both TEE and ZK proofs present with no challenge
    /// (`countered_index == 0`). The second proof was added via
    /// `verifyProposalProof`, not via `challenge`.
    ///
    /// Both proofs may still verify an incorrect root. The driver
    /// nullifies the TEE proof first (fast, synchronous) and falls back
    /// to ZK nullification if TEE proving is unavailable or fails.
    /// After TEE nullification the game becomes `(false, true, 0)` and
    /// will be re-classified as [`GameCategory::InvalidZkProposal`] on the next scan.
    InvalidDualProposal,
}

/// A dispute game that has been identified as a candidate for action.
#[derive(Debug, Clone)]
pub struct CandidateGame {
    /// The factory index of this game.
    pub index: u64,
    /// Game data from the factory contract.
    pub factory: GameAtIndex,
    /// Game info from the verifier contract.
    pub info: GameInfo,
    /// The starting block number for this game.
    pub starting_block_number: u64,
    /// The intermediate block interval for this game's type.
    pub intermediate_block_interval: u64,
    /// The L1 head block hash stored at game creation time.
    pub l1_head: B256,
    /// Address of the TEE prover for this game (`Address::ZERO` if none registered).
    pub tee_prover: Address,
    /// Classification of this candidate and the action the driver should take.
    pub category: GameCategory,
}

impl CandidateGame {
    /// Computes the starting block number for the given intermediate root index.
    pub fn checkpoint_start_block(&self, index: u64) -> eyre::Result<u64> {
        let offset = self
            .intermediate_block_interval
            .checked_mul(index)
            .ok_or_else(|| eyre::eyre!("checkpoint offset overflow"))?;
        self.starting_block_number
            .checked_add(offset)
            .ok_or_else(|| eyre::eyre!("checkpoint start block overflow"))
    }
}

/// Scans the `DisputeGameFactory` for dispute games that need validation.
///
/// The scanner is fully stateless — every call re-evaluates the entire
/// lookback window so that on-chain state changes (new proofs added,
/// challenges filed) are always detected.
pub struct GameScanner {
    factory_client: Arc<dyn DisputeGameFactoryClient>,
    verifier_client: Arc<dyn AggregateVerifierClient>,
    config: ScannerConfig,
    /// Cache of `game_proxy → intermediate_block_interval` to avoid repeated RPC calls while
    /// preserving the interval used by each game's implementation. Pruned after each scan to the
    /// candidate games still present in the current lookback window.
    interval_cache: Mutex<HashMap<Address, u64>>,
}

impl std::fmt::Debug for GameScanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameScanner").field("config", &self.config).finish_non_exhaustive()
    }
}

impl GameScanner {
    /// Maximum number of games to evaluate concurrently during a scan.
    pub const SCAN_CONCURRENCY: usize = 32;

    /// Creates a new game scanner.
    pub fn new(
        factory_client: Arc<dyn DisputeGameFactoryClient>,
        verifier_client: Arc<dyn AggregateVerifierClient>,
        config: ScannerConfig,
    ) -> Self {
        Self { factory_client, verifier_client, config, interval_cache: Mutex::new(HashMap::new()) }
    }

    /// Scans the lookback window for candidate games that need validation.
    ///
    /// Every call re-evaluates the full lookback window so that on-chain state
    /// changes (new proofs added, challenges filed) are always detected. Games
    /// that are not `IN_PROGRESS` or have been fully nullified are filtered out
    /// cheaply via a single `status()` RPC call.
    ///
    /// Individual game query failures are logged and skipped so that a transient
    /// RPC error on one game does not abort the entire scan. Errored games are
    /// naturally retried on the next tick. After evaluation, the
    /// `base_challenger_games_scanned_total` counter and
    /// `base_challenger_scan_head` gauge are updated.
    pub async fn scan(&self) -> Result<Vec<CandidateGame>> {
        let game_count = self.factory_client.game_count().await?;

        if game_count == 0 {
            debug!("factory has no games");
            return Ok(vec![]);
        }

        let end = game_count - 1;
        let start = game_count.saturating_sub(self.config.lookback_games);

        let games_to_scan = end - start + 1;

        let results: Vec<(u64, Result<Option<CandidateGame>>)> = stream::iter(start..=end)
            .map(|i| async move { (i, self.evaluate_game(i).await) })
            .buffer_unordered(Self::SCAN_CONCURRENCY)
            .collect()
            .await;

        let mut candidates = Vec::new();

        for (i, result) in results {
            match result {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => {}
                Err(e) => {
                    warn!(error = %e, index = i, "failed to query game, skipping");
                }
            }
        }

        candidates.sort_unstable_by_key(|c| c.index);
        self.prune_interval_cache(candidates.iter().map(|candidate| candidate.factory.proxy));

        ChallengerMetrics::games_scanned_total().increment(games_to_scan);
        ChallengerMetrics::scan_head().set(end as f64);

        info!(
            games_found = candidates.len(),
            scan_head = end,
            games_scanned = games_to_scan,
            "scan complete"
        );

        Ok(candidates)
    }

    /// Evaluates a single game at the given factory index.
    ///
    /// Returns `Some(CandidateGame)` if the game is `IN_PROGRESS` and
    /// matches one of the four [`GameCategory`] variants. Returns `None`
    /// if the game should be skipped (resolved, fully nullified, or in
    /// an unrecognized state).
    pub async fn evaluate_game(&self, index: u64) -> Result<Option<CandidateGame>> {
        let factory = self.factory_client.game_at_index(index).await?;

        let status = self.verifier_client.status(factory.proxy).await?;
        if status != GameStatus::InProgress {
            debug!(index = index, status = ?status, "skipping game not in progress");
            return Ok(None);
        }

        // Fetch classification fields only for in-progress games.
        let (zk_prover, tee_prover, countered_index) = tokio::try_join!(
            self.verifier_client.zk_prover(factory.proxy),
            self.verifier_client.tee_prover(factory.proxy),
            self.verifier_client.countered_index(factory.proxy),
        )?;

        let category = match Self::classify(index, tee_prover, zk_prover, countered_index) {
            Some(c) => c,
            None => return Ok(None),
        };

        // Fetch remaining fields only for actionable games.
        let ((info, starting_block_number, l1_head), intermediate_block_interval) = tokio::try_join!(
            async {
                tokio::try_join!(
                    self.verifier_client.game_info(factory.proxy),
                    self.verifier_client.starting_block_number(factory.proxy),
                    self.verifier_client.l1_head(factory.proxy),
                )
                .map_err(Into::into)
            },
            self.resolve_intermediate_block_interval(factory.proxy),
        )?;

        Ok(Some(CandidateGame {
            index,
            factory,
            info,
            starting_block_number,
            intermediate_block_interval,
            l1_head,
            tee_prover,
            category,
        }))
    }

    /// Classifies a game into a [`GameCategory`] based on its prover state,
    /// or returns `None` if the game should be skipped.
    fn classify(
        index: u64,
        tee_prover: Address,
        zk_prover: Address,
        countered_index: u64,
    ) -> Option<GameCategory> {
        let has_tee = tee_prover != Address::ZERO;
        let has_zk = zk_prover != Address::ZERO;

        match (has_tee, has_zk, countered_index) {
            // Path 1: TEE-proposed, unchallenged.
            (true, false, 0) => Some(GameCategory::InvalidTeeProposal),

            // Unreachable: `ci > 0` requires `challenge()` (which sets `zkProver`),
            // and clearing `zkProver` runs through `_proofRefutedUpdate(ZK)` which
            // also clears `ci`. Suspect contract bug if observed.
            (true, false, ci) => {
                error!(
                    index = index,
                    countered_index = ci,
                    "skipping TEE-only game with unexpected non-zero countered_index"
                );
                None
            }

            // TEE + ZK present but no countered index — second proof was added
            // via `verifyProposalProof`, not via `challenge`. Both proofs may
            // still verify an incorrect root. Nullify the TEE proof first
            // (fast) then the ZK proof on the next scan.
            (true, true, 0) => {
                debug!(index = index, "dual-proof game selected for validation");
                Some(GameCategory::InvalidDualProposal)
            }

            // Path 2: TEE-proposed and challenged by ZK.
            (true, true, ci) => {
                debug_assert!(ci > 0, "ci == 0 should be handled by (true, true, 0) arm");
                Some(GameCategory::FraudulentZkChallenge { challenged_index: ci - 1 })
            }

            // Path 3: ZK-proposed, unchallenged.
            (false, true, 0) => Some(GameCategory::InvalidZkProposal),

            // Only reachable after a global `TEE_VERIFIER.nullify()` drops the
            // TEE proof on a game with an active challenge (`_updateProofCount`
            // does not clear `ci` for TEE refutations). Requires a TEE soundness
            // break or key compromise.
            (false, true, ci) => {
                warn!(
                    index = index,
                    countered_index = ci,
                    "skipping ZK-only game with unexpected non-zero countered_index"
                );
                None
            }

            // Both provers zeroed — already nullified.
            (false, false, _) => {
                debug!(index = index, "skipping nullified game (both provers zeroed)");
                None
            }
        }
    }

    /// Resolves the intermediate block interval for a game.
    ///
    /// The interval is read from the game proxy, not the factory's current implementation for the
    /// game type, because governance can replace the factory implementation while older in-progress
    /// games still delegate to the implementation they were created from.
    async fn resolve_intermediate_block_interval(&self, game_address: Address) -> Result<u64> {
        {
            let cache = self.interval_cache.lock().expect("interval_cache lock poisoned");
            if let Some(&interval) = cache.get(&game_address) {
                return Ok(interval);
            }
        }

        let interval = self.verifier_client.read_intermediate_block_interval(game_address).await?;

        debug!(
            game_address = %game_address,
            interval = interval,
            "resolved intermediate block interval"
        );

        let mut cache = self.interval_cache.lock().expect("interval_cache lock poisoned");
        cache.insert(game_address, interval);

        Ok(interval)
    }

    fn prune_interval_cache(&self, retained_games: impl IntoIterator<Item = Address>) {
        let retained_games: HashSet<_> = retained_games.into_iter().collect();
        let mut cache = self.interval_cache.lock().expect("interval_cache lock poisoned");
        let before = cache.len();

        cache.retain(|game_address, _| retained_games.contains(game_address));

        let entries_removed = before - cache.len();
        if entries_removed > 0 {
            debug!(
                entries_removed = entries_removed,
                entries_remaining = cache.len(),
                "pruned intermediate block interval cache"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alloy_primitives::Address;
    use base_proof_contracts::{AggregateVerifierClient, DisputeGameFactoryClient, GameStatus};

    use super::{GameScanner, ScannerConfig};
    use crate::test_utils::{
        MockAggregateVerifier, MockDisputeGameFactory, addr, factory_game, mock_state,
    };

    #[tokio::test]
    async fn scanner_reads_interval_from_each_game_proxy() {
        let game_type = 1;

        let factory: Arc<dyn DisputeGameFactoryClient> = Arc::new(MockDisputeGameFactory {
            games: vec![factory_game(0, game_type), factory_game(1, game_type)],
        });

        let mut verifier_games = HashMap::new();
        verifier_games.insert(addr(0), mock_state(GameStatus::InProgress, Address::ZERO, 100));
        verifier_games.insert(addr(1), mock_state(GameStatus::InProgress, Address::ZERO, 200));

        let verifier = Arc::new(MockAggregateVerifier::new(verifier_games));
        verifier.set_intermediate_block_interval(addr(0), 10);
        verifier.set_intermediate_block_interval(addr(1), 30);
        let verifier_client: Arc<dyn AggregateVerifierClient> =
            Arc::<MockAggregateVerifier>::clone(&verifier);

        let scanner =
            GameScanner::new(factory, verifier_client, ScannerConfig { lookback_games: 1000 });

        let first = scanner.evaluate_game(0).await.unwrap().unwrap();
        assert_eq!(first.intermediate_block_interval, 10);

        let cached = scanner.evaluate_game(0).await.unwrap().unwrap();
        assert_eq!(cached.intermediate_block_interval, 10);
        assert_eq!(verifier.intermediate_block_interval_read_count(addr(0)), 1);

        let after_upgrade = scanner.evaluate_game(1).await.unwrap().unwrap();
        assert_eq!(after_upgrade.intermediate_block_interval, 30);
        assert_eq!(verifier.intermediate_block_interval_read_count(addr(1)), 1);
    }

    #[tokio::test]
    async fn scan_prunes_interval_cache_to_current_candidates() {
        let factory: Arc<dyn DisputeGameFactoryClient> = Arc::new(MockDisputeGameFactory {
            games: vec![
                factory_game(0, 1),
                factory_game(1, 1),
                factory_game(2, 1),
                factory_game(3, 1),
            ],
        });

        let mut verifier_games = HashMap::new();
        for i in 0..4 {
            verifier_games.insert(addr(i), mock_state(GameStatus::InProgress, Address::ZERO, i));
        }

        let verifier = Arc::new(MockAggregateVerifier::new(verifier_games));
        let verifier_client: Arc<dyn AggregateVerifierClient> =
            Arc::<MockAggregateVerifier>::clone(&verifier);

        let scanner =
            GameScanner::new(factory, verifier_client, ScannerConfig { lookback_games: 2 });

        let old_candidate = scanner.evaluate_game(0).await.unwrap().unwrap();
        assert_eq!(old_candidate.index, 0);
        assert!(scanner.interval_cache.lock().unwrap().contains_key(&addr(0)));

        let candidates = scanner.scan().await.unwrap();
        assert_eq!(
            candidates.iter().map(|candidate| candidate.index).collect::<Vec<_>>(),
            vec![2, 3]
        );

        let cache = scanner.interval_cache.lock().unwrap();
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains_key(&addr(0)));
        assert!(cache.contains_key(&addr(2)));
        assert!(cache.contains_key(&addr(3)));
    }
}
