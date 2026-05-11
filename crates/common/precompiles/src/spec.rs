use base_common_chains::BaseUpgrade;

/// A chain spec that can select Base precompile sets.
pub trait BasePrecompileSpec: Copy + Eq {
    /// Returns the default precompile spec.
    fn default_precompile_spec() -> Self;

    /// Returns the Base upgrade associated with this spec.
    fn upgrade(self) -> BaseUpgrade;
}

impl BasePrecompileSpec for BaseUpgrade {
    fn default_precompile_spec() -> Self {
        Self::Jovian
    }

    fn upgrade(self) -> BaseUpgrade {
        self
    }
}
