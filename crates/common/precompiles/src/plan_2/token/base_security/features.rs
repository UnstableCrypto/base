//! SecurityFeatures bitmap for BaseSecurity. Append-only.

pub const SECURITY_MEMO: u8 = 1 << 0;
pub const SECURITY_FORCE_TRANSFER: u8 = 1 << 1;
pub const SECURITY_HOLDER_LIMIT: u8 = 1 << 2;
pub const SECURITY_ALL_KNOWN: u8 = SECURITY_MEMO | SECURITY_FORCE_TRANSFER | SECURITY_HOLDER_LIMIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityFeatures(pub u8);

impl SecurityFeatures {
    #[inline]
    pub const fn new(bits: u8) -> Self { Self(bits) }
    #[inline]
    pub const fn has(&self, bit: u8) -> bool { (self.0 & bit) != 0 }
    #[inline]
    pub const fn bits(&self) -> u8 { self.0 }
}
