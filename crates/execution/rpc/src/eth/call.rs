use reth_rpc_eth_api::{
    FromEvmError, RpcConvert,
    helpers::{Call, EthCall, estimate::EstimateCall},
};

use crate::{UnstableEthApi, UnstableEthApiError, eth::RpcNodeCore};

impl<N, Rpc> EthCall for UnstableEthApi<N, Rpc>
where
    N: RpcNodeCore,
    UnstableEthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = UnstableEthApiError, Evm = N::Evm>,
{
}

impl<N, Rpc> EstimateCall for UnstableEthApi<N, Rpc>
where
    N: RpcNodeCore,
    UnstableEthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = UnstableEthApiError, Evm = N::Evm>,
{
}

impl<N, Rpc> Call for UnstableEthApi<N, Rpc>
where
    N: RpcNodeCore,
    UnstableEthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = UnstableEthApiError, Evm = N::Evm>,
{
    #[inline]
    fn call_gas_limit(&self) -> u64 {
        self.inner.eth_api.gas_cap()
    }

    #[inline]
    fn max_simulate_blocks(&self) -> u64 {
        self.inner.eth_api.max_simulate_blocks()
    }

    #[inline]
    fn evm_memory_limit(&self) -> u64 {
        self.inner.eth_api.evm_memory_limit()
    }
}
