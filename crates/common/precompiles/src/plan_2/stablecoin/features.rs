//! StablecoinFeatures bitmap for BaseStablecoin. Append-only.

pub const STABLECOIN_MEMO: u8 = 1 << 0;
pub const STABLECOIN_ALL_KNOWN: u8 = STABLECOIN_MEMO;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StablecoinFeatures(pub u8);

impl StablecoinFeatures {
    #[inline]
    pub const fn new(bits: u8) -> Self { Self(bits) }
    #[inline]
    pub const fn has(&self, bit: u8) -> bool { (self.0 & bit) != 0 }
    #[inline]
    pub const fn bits(&self) -> u8 { self.0 }
}
