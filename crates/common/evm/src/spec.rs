//! Contains the `[UnstableSpecId]` type and its implementation.

use alloy_consensus::BlockHeader;
use base_common_chains::{UnstableUpgrade, Upgrades};
use revm::primitives::hardfork::SpecId;

/// EVM-facing Unstable spec id.
///
/// This wraps the canonical Unstable upgrade type and adds revm-specific behavior.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UnstableSpecId(UnstableUpgrade);

impl UnstableSpecId {
    /// Creates a new Unstable EVM spec id for the given Unstable upgrade.
    pub const fn new(upgrade: UnstableUpgrade) -> Self {
        Self(upgrade)
    }

    /// Returns the wrapped Unstable upgrade.
    pub const fn upgrade(self) -> UnstableUpgrade {
        self.0
    }

    /// Converts the [`UnstableSpecId`] into a [`SpecId`].
    pub const fn into_eth_spec(self) -> SpecId {
        self.0.into_eth_spec()
    }

    /// Checks if the given Unstable upgrade is enabled in this spec.
    pub const fn is_enabled_in(self, other: UnstableUpgrade) -> bool {
        other as u8 <= self.0 as u8
    }

    /// Parses the [`UnstableSpecId`] from the chain spec and block header.
    pub fn from_header(chain_spec: impl Upgrades, header: impl BlockHeader) -> Self {
        Self::from_timestamp(chain_spec, header.timestamp())
    }

    /// Returns the [`UnstableSpecId`] at the given timestamp.
    ///
    /// # Note
    ///
    /// This is only intended to be used after the Bedrock, when hardforks are activated by
    /// timestamp.
    pub fn from_timestamp(chain_spec: impl Upgrades, timestamp: u64) -> Self {
        Self(UnstableUpgrade::from_timestamp(chain_spec, timestamp))
    }
}

impl From<UnstableUpgrade> for UnstableSpecId {
    fn from(upgrade: UnstableUpgrade) -> Self {
        Self(upgrade)
    }
}

impl From<UnstableSpecId> for SpecId {
    fn from(spec: UnstableSpecId) -> Self {
        spec.into_eth_spec()
    }
}

impl From<UnstableSpecId> for UnstableUpgrade {
    fn from(spec: UnstableSpecId) -> Self {
        spec.upgrade()
    }
}

impl From<UnstableSpecId> for &'static str {
    fn from(spec: UnstableSpecId) -> Self {
        spec.0.name()
    }
}

impl core::fmt::Display for UnstableSpecId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl core::str::FromStr for UnstableSpecId {
    type Err = <UnstableUpgrade as core::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<UnstableUpgrade>().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn test_base_spec_id_eth_spec_compatibility() {
        // Define test cases: (UnstableUpgrade, enabled in ETH specs, enabled in Unstable upgrades)
        let test_cases = [
            (
                UnstableUpgrade::Bedrock,
                vec![
                    (SpecId::MERGE, true),
                    (SpecId::SHANGHAI, false),
                    (SpecId::CANCUN, false),
                    (SpecId::default(), false),
                ],
                vec![(UnstableUpgrade::Bedrock, true), (UnstableUpgrade::Regolith, false)],
            ),
            (
                UnstableUpgrade::Regolith,
                vec![
                    (SpecId::MERGE, true),
                    (SpecId::SHANGHAI, false),
                    (SpecId::CANCUN, false),
                    (SpecId::default(), false),
                ],
                vec![(UnstableUpgrade::Bedrock, true), (UnstableUpgrade::Regolith, true)],
            ),
            (
                UnstableUpgrade::Canyon,
                vec![
                    (SpecId::MERGE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, false),
                    (SpecId::default(), false),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                ],
            ),
            (
                UnstableUpgrade::Ecotone,
                vec![
                    (SpecId::MERGE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, true),
                    (SpecId::default(), false),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                    (UnstableUpgrade::Ecotone, true),
                ],
            ),
            (
                UnstableUpgrade::Fjord,
                vec![
                    (SpecId::MERGE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, true),
                    (SpecId::default(), false),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                    (UnstableUpgrade::Ecotone, true),
                    (UnstableUpgrade::Fjord, true),
                ],
            ),
            (
                UnstableUpgrade::Jovian,
                vec![
                    (SpecId::PRAGUE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, true),
                    (SpecId::MERGE, true),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                    (UnstableUpgrade::Ecotone, true),
                    (UnstableUpgrade::Fjord, true),
                    (UnstableUpgrade::Holocene, true),
                    (UnstableUpgrade::Isthmus, true),
                ],
            ),
            (
                UnstableUpgrade::Azul,
                vec![
                    (SpecId::OSAKA, true),
                    (SpecId::PRAGUE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, true),
                    (SpecId::MERGE, true),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                    (UnstableUpgrade::Ecotone, true),
                    (UnstableUpgrade::Fjord, true),
                    (UnstableUpgrade::Holocene, true),
                    (UnstableUpgrade::Isthmus, true),
                    (UnstableUpgrade::Jovian, true),
                ],
            ),
            (
                UnstableUpgrade::Beryl,
                vec![
                    (SpecId::OSAKA, true),
                    (SpecId::PRAGUE, true),
                    (SpecId::SHANGHAI, true),
                    (SpecId::CANCUN, true),
                    (SpecId::MERGE, true),
                ],
                vec![
                    (UnstableUpgrade::Bedrock, true),
                    (UnstableUpgrade::Regolith, true),
                    (UnstableUpgrade::Canyon, true),
                    (UnstableUpgrade::Ecotone, true),
                    (UnstableUpgrade::Fjord, true),
                    (UnstableUpgrade::Holocene, true),
                    (UnstableUpgrade::Isthmus, true),
                    (UnstableUpgrade::Jovian, true),
                    (UnstableUpgrade::Azul, true),
                ],
            ),
        ];

        for (base_upgrade, eth_tests, base_tests) in test_cases {
            let base_spec = UnstableSpecId::new(base_upgrade);

            // Test ETH spec compatibility
            for (eth_spec, expected) in eth_tests {
                assert_eq!(
                    base_spec.into_eth_spec().is_enabled_in(eth_spec),
                    expected,
                    "{base_spec:?} should {} be enabled in ETH {eth_spec:?}",
                    if expected { "" } else { "not " },
                );
            }

            // Test Unstable upgrade compatibility
            for (other_base_upgrade, expected) in base_tests {
                assert_eq!(
                    base_spec.is_enabled_in(other_base_upgrade),
                    expected,
                    "{base_spec:?} should {} be enabled in Unstable {other_base_upgrade:?}",
                    if expected { "" } else { "not " },
                );
            }
        }
    }

    #[test]
    fn default_base_spec_id() {
        assert_eq!(UnstableSpecId::default().upgrade(), UnstableUpgrade::LATEST);
    }
}
