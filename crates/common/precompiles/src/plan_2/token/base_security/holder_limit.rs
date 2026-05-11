//! Holder limit tracking for BaseSecurity. Gated by SECURITY_HOLDER_LIMIT bit.

use alloy::primitives::{Address, U256};
use base_precompiles_contracts::{BaseSecurityError, IBaseSecurity};

use crate::{
    error::{BasePrecompileError, Result},
    plan_2::shared::TransferKind,
    plan_2::token::base_security::BaseSecurity,
    storage::Handler,
};

impl BaseSecurity {
    pub fn holder_count(&self, _call: IBaseSecurity::holderCountCall) -> Result<u64> {
        self.extra.holder_count.read()
    }

    pub fn holder_limit(&self, _call: IBaseSecurity::holderLimitCall) -> Result<u64> {
        self.extra.holder_limit_cap.read()
    }

    pub(super) fn update_holder_count(
        &mut self, from: Address, to: Address, amount: U256, kind: TransferKind,
    ) -> Result<()> {
        let zero = U256::ZERO;
        let mut count = self.extra.holder_count.read()?;

        if matches!(kind, TransferKind::Transfer | TransferKind::Mint) {
            let prev_to = self.core.get_balance(to)?.checked_sub(amount).unwrap_or(zero);
            if prev_to == zero {
                let limit = self.extra.holder_limit_cap.read()?;
                count = count.checked_add(1).ok_or(BasePrecompileError::under_overflow())?;
                if limit > 0 && count > limit {
                    return Err(BasePrecompileError::BaseSecurity(
                        BaseSecurityError::holder_limit_reached(),
                    ));
                }
                self.extra.holder_count.write(count)?;
            }
        }

        if matches!(kind, TransferKind::Transfer | TransferKind::Burn) {
            if self.core.get_balance(from)? == zero {
                count = count.saturating_sub(1);
                self.extra.holder_count.write(count)?;
            }
        }

        Ok(())
    }
}
