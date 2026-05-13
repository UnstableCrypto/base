use alloy_consensus::ReceiptWithBloom;
use alloy_network::Network;
use alloy_provider::fillers::{
    ChainIdFiller, GasFiller, JoinFill, NonceFiller, RecommendedFillers,
};
use base_common_consensus::{UnstableReceipt, OpTxType};

/// Types for a Unstable chain network.
#[derive(Clone, Copy, Debug)]
pub struct Unstable {
    _private: (),
}

impl Network for Unstable {
    type TxType = OpTxType;

    type TxEnvelope = base_common_consensus::UnstableTxEnvelope;

    type UnsignedTx = base_common_consensus::UnstableTypedTransaction;

    type ReceiptEnvelope = ReceiptWithBloom<UnstableReceipt>;

    type Header = alloy_consensus::Header;

    type TransactionRequest = base_common_rpc_types::UnstableTransactionRequest;

    type TransactionResponse = base_common_rpc_types::Transaction;

    type ReceiptResponse = base_common_rpc_types::UnstableTransactionReceipt;

    type HeaderResponse = alloy_rpc_types_eth::Header;

    type BlockResponse =
        alloy_rpc_types_eth::Block<Self::TransactionResponse, Self::HeaderResponse>;
}

impl RecommendedFillers for Unstable {
    type RecommendedFillers = JoinFill<GasFiller, JoinFill<NonceFiller, ChainIdFiller>>;

    fn recommended_fillers() -> Self::RecommendedFillers {
        Default::default()
    }
}
