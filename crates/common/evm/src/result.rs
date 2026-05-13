//! Contains the `[UnstableHaltReason]` type.
use revm::context_interface::result::HaltReason;

/// Unstable halt reason.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UnstableHaltReason {
    /// Unstable halt reason.
    Unstable(HaltReason),
    /// Failed deposit halt reason.
    FailedDeposit,
}

impl From<HaltReason> for UnstableHaltReason {
    fn from(value: HaltReason) -> Self {
        Self::Unstable(value)
    }
}

impl TryFrom<UnstableHaltReason> for HaltReason {
    type Error = UnstableHaltReason;

    fn try_from(value: UnstableHaltReason) -> Result<Self, UnstableHaltReason> {
        match value {
            UnstableHaltReason::Unstable(reason) => Ok(reason),
            UnstableHaltReason::FailedDeposit => Err(value),
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use revm::context_interface::result::OutOfGasError;

    use super::*;

    #[test]
    fn test_serialize_json_base_halt_reason() {
        let response = r#"{"Unstable":{"OutOfGas":"Basic"}}"#;

        let base_halt_reason: UnstableHaltReason = serde_json::from_str(response).unwrap();
        assert_eq!(base_halt_reason, HaltReason::OutOfGas(OutOfGasError::Basic).into());
    }
}
