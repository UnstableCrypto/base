use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::TransactionId;

/// A transaction with its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction identifier.
    pub id: TransactionId,
    /// Raw transaction data.
    pub data: Bytes,
}
