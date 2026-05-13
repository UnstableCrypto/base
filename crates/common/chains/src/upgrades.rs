use alloy_hardforks::{EthereumHardforks, ForkCondition};
use base_common_genesis::RollupConfig;

use crate::UnstableUpgrade;

/// Extends [`EthereumHardforks`] with Unstable upgrade helper methods.
#[auto_impl::auto_impl(&, Arc)]
pub trait Upgrades: EthereumHardforks {
    /// Retrieves [`ForkCondition`] by a [`UnstableUpgrade`]. If `fork` is not present, returns
    /// [`ForkCondition::Never`].
    fn upgrade_activation(&self, fork: UnstableUpgrade) -> ForkCondition;

    /// Convenience method to check if [`UnstableUpgrade::Bedrock`] is active at a given block
    /// number.
    fn is_bedrock_active_at_block(&self, block_number: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Bedrock).active_at_block(block_number)
    }

    /// Returns `true` if [`Regolith`](UnstableUpgrade::Regolith) is active at given block
    /// timestamp.
    fn is_regolith_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Regolith).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Canyon`](UnstableUpgrade::Canyon) is active at given block timestamp.
    fn is_canyon_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Canyon).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Ecotone`](UnstableUpgrade::Ecotone) is active at given block timestamp.
    fn is_ecotone_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Ecotone).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Fjord`](UnstableUpgrade::Fjord) is active at given block timestamp.
    fn is_fjord_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Fjord).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Granite`](UnstableUpgrade::Granite) is active at given block timestamp.
    fn is_granite_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Granite).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Holocene`](UnstableUpgrade::Holocene) is active at given block
    /// timestamp.
    fn is_holocene_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Holocene).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Isthmus`](UnstableUpgrade::Isthmus) is active at given block
    /// timestamp.
    fn is_isthmus_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Isthmus).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Jovian`](UnstableUpgrade::Jovian) is active at given block
    /// timestamp.
    fn is_jovian_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Jovian).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Azul`](UnstableUpgrade::Azul) is active at given block timestamp.
    fn is_azul_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Azul).active_at_timestamp(timestamp)
    }

    /// Returns `true` if [`Beryl`](UnstableUpgrade::Beryl) is active at given block timestamp.
    fn is_beryl_active_at_timestamp(&self, timestamp: u64) -> bool {
        self.upgrade_activation(UnstableUpgrade::Beryl).active_at_timestamp(timestamp)
    }
}

impl Upgrades for RollupConfig {
    fn upgrade_activation(&self, fork: UnstableUpgrade) -> ForkCondition {
        match fork {
            UnstableUpgrade::Bedrock => ForkCondition::Block(0),
            UnstableUpgrade::Regolith => self
                .hardforks
                .regolith_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Canyon)),
            UnstableUpgrade::Canyon => self
                .hardforks
                .canyon_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Ecotone)),
            UnstableUpgrade::Ecotone => self
                .hardforks
                .ecotone_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Fjord)),
            UnstableUpgrade::Fjord => self
                .hardforks
                .fjord_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Granite)),
            UnstableUpgrade::Granite => self
                .hardforks
                .granite_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Holocene)),
            UnstableUpgrade::Holocene => self
                .hardforks
                .holocene_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Isthmus)),
            UnstableUpgrade::Isthmus => self
                .hardforks
                .isthmus_time
                .map(ForkCondition::Timestamp)
                .unwrap_or_else(|| self.upgrade_activation(UnstableUpgrade::Jovian)),
            UnstableUpgrade::Jovian => self
                .hardforks
                .jovian_time
                .map(ForkCondition::Timestamp)
                .unwrap_or(ForkCondition::Never),
            UnstableUpgrade::Azul => self
                .hardforks
                .base
                .azul
                .map(ForkCondition::Timestamp)
                .unwrap_or(ForkCondition::Never),
            UnstableUpgrade::Beryl => self
                .hardforks
                .base
                .beryl
                .map(ForkCondition::Timestamp)
                .unwrap_or(ForkCondition::Never),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollup_config_upgrade_activation_cascade() {
        const ACTIVATION: u64 = 10;
        let mut cfg = RollupConfig::default();
        cfg.hardforks.ecotone_time = Some(ACTIVATION);

        // Cascading: Regolith and Canyon should fall through to Ecotone.
        assert_eq!(
            cfg.upgrade_activation(UnstableUpgrade::Regolith),
            ForkCondition::Timestamp(ACTIVATION)
        );
        assert_eq!(
            cfg.upgrade_activation(UnstableUpgrade::Canyon),
            ForkCondition::Timestamp(ACTIVATION)
        );
        assert_eq!(
            cfg.upgrade_activation(UnstableUpgrade::Ecotone),
            ForkCondition::Timestamp(ACTIVATION)
        );

        // Bedrock is always at block 0; later forks unset are Never.
        assert_eq!(cfg.upgrade_activation(UnstableUpgrade::Bedrock), ForkCondition::Block(0));
        assert_eq!(cfg.upgrade_activation(UnstableUpgrade::Jovian), ForkCondition::Never);
        assert_eq!(cfg.upgrade_activation(UnstableUpgrade::Azul), ForkCondition::Never);
        assert_eq!(cfg.upgrade_activation(UnstableUpgrade::Beryl), ForkCondition::Never);
    }
}
