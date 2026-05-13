use alloc::{boxed::Box, vec};

use alloy_primitives::U256;
use base_common_chains::{UnstableUpgrade, ChainUpgrades};
use reth_ethereum_forks::{ChainHardforks, EthereumHardfork, ForkCondition, Hardfork};
/// Extension trait to convert alloy's [`ChainUpgrades`] into reth's [`ChainHardforks`].
pub trait ChainUpgradesExt {
    /// Expands Unstable upgrades into a full [`ChainHardforks`] including implied Ethereum entries.
    ///
    /// Pre-Bedrock Ethereum hardforks are set to block 0. Paired Ethereum hardforks
    /// use their Unstable counterpart's timestamp:
    /// Shanghai=Canyon, Cancun=Ecotone, Prague=Isthmus, Osaka=Azul.
    fn to_chain_hardforks(&self) -> ChainHardforks;
}

impl ChainUpgradesExt for ChainUpgrades {
    fn to_chain_hardforks(&self) -> ChainHardforks {
        let mut forks: vec::Vec<(Box<dyn Hardfork>, ForkCondition)> = vec![
            (EthereumHardfork::Frontier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Homestead.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Tangerine.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::SpuriousDragon.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Byzantium.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Constantinople.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Petersburg.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Istanbul.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::MuirGlacier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::Berlin.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::London.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::ArrowGlacier.boxed(), ForkCondition::Block(0)),
            (EthereumHardfork::GrayGlacier.boxed(), ForkCondition::Block(0)),
            (
                EthereumHardfork::Paris.boxed(),
                ForkCondition::TTD {
                    activation_block_number: 0,
                    fork_block: Some(0),
                    total_difficulty: U256::ZERO,
                },
            ),
        ];

        forks.push((UnstableUpgrade::Bedrock.boxed(), self[UnstableUpgrade::Bedrock]));
        forks.push((UnstableUpgrade::Regolith.boxed(), self[UnstableUpgrade::Regolith]));

        let canyon = self[UnstableUpgrade::Canyon];
        forks.push((EthereumHardfork::Shanghai.boxed(), canyon));
        forks.push((UnstableUpgrade::Canyon.boxed(), canyon));

        let ecotone = self[UnstableUpgrade::Ecotone];
        forks.push((EthereumHardfork::Cancun.boxed(), ecotone));
        forks.push((UnstableUpgrade::Ecotone.boxed(), ecotone));

        forks.push((UnstableUpgrade::Fjord.boxed(), self[UnstableUpgrade::Fjord]));
        forks.push((UnstableUpgrade::Granite.boxed(), self[UnstableUpgrade::Granite]));
        forks.push((UnstableUpgrade::Holocene.boxed(), self[UnstableUpgrade::Holocene]));

        let isthmus = self[UnstableUpgrade::Isthmus];
        if !matches!(isthmus, ForkCondition::Never) {
            forks.push((EthereumHardfork::Prague.boxed(), isthmus));
            forks.push((UnstableUpgrade::Isthmus.boxed(), isthmus));
        }

        let jovian = self[UnstableUpgrade::Jovian];
        if !matches!(jovian, ForkCondition::Never) {
            forks.push((UnstableUpgrade::Jovian.boxed(), jovian));
        }

        let azul = self[UnstableUpgrade::Azul];
        if !matches!(azul, ForkCondition::Never) {
            forks.push((EthereumHardfork::Osaka.boxed(), azul));
            forks.push((UnstableUpgrade::Azul.boxed(), azul));
        }

        let beryl = self[UnstableUpgrade::Beryl];
        if !matches!(beryl, ForkCondition::Never) {
            forks.push((UnstableUpgrade::Beryl.boxed(), beryl));
        }

        ChainHardforks::new(forks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azul_expands_to_osaka() {
        let hardforks =
            ChainUpgrades::new(UnstableUpgrade::devnet().into_iter().map(|(fork, cond)| {
                if fork == UnstableUpgrade::Azul {
                    (fork, ForkCondition::Timestamp(1_000_000))
                } else {
                    (fork, cond)
                }
            }))
            .to_chain_hardforks();
        assert_eq!(hardforks.get(UnstableUpgrade::Azul), Some(ForkCondition::Timestamp(1_000_000)));
        assert_eq!(hardforks.get(EthereumHardfork::Osaka), hardforks.get(UnstableUpgrade::Azul));
    }
}
