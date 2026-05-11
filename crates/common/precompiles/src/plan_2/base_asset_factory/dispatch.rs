//! ABI dispatch for [`BaseAssetFactory`].

use alloy::{primitives::Address, sol_types::SolInterface};
use base_precompiles_contracts::IBaseAssetFactory::IBaseAssetFactoryCalls;
use revm::precompile::PrecompileResult;

use crate::{
    Precompile, charge_input_cost, dispatch_call, mutate, view,
    plan_2::base_asset_factory::BaseAssetFactory,
};

impl Precompile for BaseAssetFactory {
    fn call(&mut self, calldata: &[u8], msg_sender: Address) -> PrecompileResult {
        if let Some(err) = charge_input_cost(&mut self.storage, calldata) {
            return err;
        }

        dispatch_call(calldata, &[], IBaseAssetFactoryCalls::abi_decode, |call| match call {
            IBaseAssetFactoryCalls::createBaseAsset(c) => {
                mutate(c, msg_sender, |s, c| self.create_base_asset(s, c))
            }
            IBaseAssetFactoryCalls::getBaseAssetAddress(c) => {
                view(c, |c| self.get_base_asset_address(c))
            }
            IBaseAssetFactoryCalls::isBaseAsset(c) => view(c, |c| self.is_base_asset(c.token)),
        })
    }
}
