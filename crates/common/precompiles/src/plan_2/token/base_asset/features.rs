//! AssetFeatures bitmap for BaseAsset. Append-only.

pub const ASSET_SUPPLY_CONTROL: u8 = 1 << 0;
pub const ASSET_MEMO: u8 = 1 << 1;
pub const ASSET_SUPPLY_CAP: u8 = 1 << 2;
pub const ASSET_ALL_KNOWN: u8 = ASSET_SUPPLY_CONTROL | ASSET_MEMO | ASSET_SUPPLY_CAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetFeatures(pub u8);

impl AssetFeatures {
    #[inline]
    pub const fn new(bits: u8) -> Self { Self(bits) }
    #[inline]
    pub const fn has(&self, bit: u8) -> bool { (self.0 & bit) != 0 }
    #[inline]
    pub const fn bits(&self) -> u8 { self.0 }
}
