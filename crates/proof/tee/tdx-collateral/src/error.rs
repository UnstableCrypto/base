//! Error type for Intel TDX collateral hydration.

use std::error::Error;

use thiserror::Error;

/// Boxed error type used by TDX collateral hydration.
pub type BoxError = Box<dyn Error + Send + Sync>;

/// TDX collateral hydration error.
#[derive(Debug, Error)]
#[error("{source}")]
pub struct TdxCollateralError {
    /// Underlying hydration, parsing, validation, or HTTP error.
    #[source]
    pub source: BoxError,
}

impl TdxCollateralError {
    /// Creates a collateral error from an underlying source.
    pub fn source(source: impl Into<BoxError>) -> Self {
        Self { source: source.into() }
    }
}

/// Convenience result alias for TDX collateral hydration.
pub type Result<T, E = TdxCollateralError> = std::result::Result<T, E>;
