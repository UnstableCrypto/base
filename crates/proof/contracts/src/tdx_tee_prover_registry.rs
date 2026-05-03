//! `TDXTEEProverRegistry` contract bindings.
//!
//! Used by the registrar to construct TDX signer registration calldata.

use alloy_sol_types::sol;

sol! {
    /// `TDXTEEProverRegistry` TDX registration interface.
    interface ITDXTEEProverRegistry {
        /// Registers a signer using a ZK proof of Intel TDX DCAP quote verification.
        function registerTDXSigner(
            bytes calldata output,
            bytes calldata proofBytes
        )
            external;
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Bytes, keccak256};
    use alloy_sol_types::SolCall;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    const TDX_TEE_PROVER_REGISTRY_ABI: &str = r#"
[
  {
    "inputs": [
      {
        "internalType": "bytes",
        "name": "output",
        "type": "bytes"
      },
      {
        "internalType": "bytes",
        "name": "proofBytes",
        "type": "bytes"
      }
    ],
    "name": "registerTDXSigner",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
  }
]
"#;

    #[rstest]
    fn register_tdx_signer_selector_matches_compiled_solidity_abi() {
        let abi = serde_json::from_str::<Value>(TDX_TEE_PROVER_REGISTRY_ABI)
            .expect("TDX registry ABI fixture must parse");
        let function = abi
            .as_array()
            .expect("ABI fixture must be an array")
            .iter()
            .find(|entry| entry["name"] == "registerTDXSigner")
            .expect("compiled ABI must contain registerTDXSigner");

        let inputs = function["inputs"].as_array().expect("function inputs must be an array");
        let input_types = inputs
            .iter()
            .map(|input| input["type"].as_str().expect("input must have ABI type"))
            .collect::<Vec<_>>();
        let signature =
            format!("{}({})", function["name"].as_str().unwrap(), input_types.join(","));
        let selector = &keccak256(signature.as_bytes())[..4];

        assert_eq!(signature, "registerTDXSigner(bytes,bytes)");
        assert_eq!(selector, ITDXTEEProverRegistry::registerTDXSignerCall::SELECTOR);
    }

    #[rstest]
    fn register_tdx_signer_abi_encodes_correctly() {
        let call = ITDXTEEProverRegistry::registerTDXSignerCall {
            output: Bytes::new(),
            proofBytes: Bytes::new(),
        };
        let encoded = call.abi_encode();

        assert_eq!(encoded.len(), 132);
        assert_eq!(&encoded[..4], &ITDXTEEProverRegistry::registerTDXSignerCall::SELECTOR);
    }
}
