use std::sync::Arc;

use base_common_chains::ChainConfig;
use base_execution_chainspec::UnstableChainSpec;
use reth_cli::chainspec::{ChainSpecParser, parse_genesis};

/// Unstable chain specification parser.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct UnstableChainSpecParser;

impl ChainSpecParser for UnstableChainSpecParser {
    type ChainSpec = UnstableChainSpec;

    const SUPPORTED_CHAINS: &'static [&'static str] = ChainConfig::SUPPORTED_NAMES;

    fn parse(s: &str) -> eyre::Result<Arc<Self::ChainSpec>> {
        chain_value_parser(s)
    }
}

/// Clap value parser for [`UnstableChainSpec`]s.
///
/// The value parser matches either a known chain, the path
/// to a json file, or a json formatted string in-memory. The json needs to be a Genesis struct.
pub fn chain_value_parser(s: &str) -> eyre::Result<Arc<UnstableChainSpec>, eyre::Error> {
    if let Some(base_chain_spec) = UnstableChainSpec::parse_chain(s) {
        Ok(base_chain_spec)
    } else {
        Ok(Arc::new(parse_genesis(s)?.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_chain_spec() {
        for &chain in UnstableChainSpecParser::SUPPORTED_CHAINS {
            assert!(
                <UnstableChainSpecParser as ChainSpecParser>::parse(chain).is_ok(),
                "Failed to parse {chain}"
            );
        }
    }
}
