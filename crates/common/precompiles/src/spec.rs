use base_common_chains::UnstableUpgrade;

/// A chain spec that can select Unstable precompile sets.
pub trait UnstablePrecompileSpec: Copy + Eq + From<UnstableUpgrade> + Into<UnstableUpgrade> {
    /// Returns the default precompile spec.
    fn default_precompile_spec() -> Self {
        UnstableUpgrade::LATEST.into()
    }

    /// Returns the Unstable upgrade associated with this spec.
    fn upgrade(self) -> UnstableUpgrade {
        self.into()
    }
}

impl<S> UnstablePrecompileSpec for S where S: Copy + Eq + From<UnstableUpgrade> + Into<UnstableUpgrade> {}
