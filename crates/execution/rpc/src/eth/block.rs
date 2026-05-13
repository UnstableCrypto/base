//! Loads and formats Unstable block RPC response.

use reth_rpc_eth_api::{
    FromEvmError, RpcConvert,
    helpers::{EthBlocks, LoadBlock},
};

use crate::{UnstableEthApi, UnstableEthApiError, eth::RpcNodeCore};

impl<N, Rpc> EthBlocks for UnstableEthApi<N, Rpc>
where
    N: RpcNodeCore,
    UnstableEthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = UnstableEthApiError>,
{
}

impl<N, Rpc> LoadBlock for UnstableEthApi<N, Rpc>
where
    N: RpcNodeCore,
    UnstableEthApiError: FromEvmError<N::Evm>,
    Rpc: RpcConvert<Primitives = N::Primitives, Error = UnstableEthApiError>,
{
}
