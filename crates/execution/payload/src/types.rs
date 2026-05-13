use base_common_consensus::UnstablePrimitives;
use base_common_rpc_types_engine::{UnstablePayloadAttributes, ExecutionData};
use reth_payload_primitives::{BuiltPayload, PayloadTypes};
use reth_primitives_traits::{Block, NodePrimitives, SealedBlock};

use crate::{UnstableBuiltPayload, UnstablePayloadBuilderAttributes};

/// ZST that aggregates Unstable [`PayloadTypes`].
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct UnstablePayloadTypes<N: NodePrimitives = UnstablePrimitives>(core::marker::PhantomData<N>);

impl<N: NodePrimitives> PayloadTypes for UnstablePayloadTypes<N>
where
    UnstableBuiltPayload<N>: BuiltPayload,
{
    type ExecutionData = ExecutionData;
    type BuiltPayload = UnstableBuiltPayload<N>;
    type PayloadAttributes = UnstablePayloadAttributes;
    type PayloadBuilderAttributes = UnstablePayloadBuilderAttributes<N::SignedTx>;

    fn block_to_payload(
        block: SealedBlock<
            <<Self::BuiltPayload as BuiltPayload>::Primitives as NodePrimitives>::Block,
        >,
    ) -> Self::ExecutionData {
        ExecutionData::from_block_unchecked(block.hash(), &block.into_block().into_ethereum_block())
    }
}
