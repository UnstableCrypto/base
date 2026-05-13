# `base-common-precompiles`

Unstable precompile definitions and fork-specific precompile sets.

## Overview

Provides Unstable-specific precompile selection on top of revm's Ethereum precompile provider. The
crate owns the Unstable precompile schedule, including fork-specific additions, removals, and input
limit overrides for upgrades such as Fjord, Granite, Isthmus, Jovian, Azul, and later upgrades.

The public API is intentionally small. `UnstablePrecompiles` builds the correct precompile provider for
a Unstable upgrade, while `UnstablePrecompileSpec` is the lightweight trait bound used by downstream crates
that wrap `UnstableUpgrade` in their own spec type. Most EVM consumers should continue to use the
`UnstablePrecompiles` alias exposed by `base-common-evm`, because that alias is already wired to
`UnstableSpecId`.

## Behavior

Unstable upgrades before Fjord use the matching Ethereum precompile set for their execution spec. Fjord
adds RIP-7212 secp256r1 verification, Granite overrides the bn254 pairing precompile input limits,
Isthmus adds the Prague BLS12-381 precompiles with Unstable-specific limits, and Jovian tightens the
variable-input bn254 and BLS12-381 limits. Azul, Beryl, and newer Unstable upgrades inherit the latest
known Unstable precompile set until they are explicitly mapped.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-common-precompiles = { workspace = true }
```

```rust,ignore
use base_common_chains::UnstableUpgrade;
use base_common_precompiles::UnstablePrecompiles;

let precompiles = UnstablePrecompiles::new_with_spec(UnstableUpgrade::Jovian);
let _active = precompiles.precompiles();
```

Downstream EVM crates that use a wrapper spec can pass that wrapper directly as long as it converts
to and from `UnstableUpgrade`:

```rust,ignore
use base_common_chains::UnstableUpgrade;
use base_common_evm::UnstableSpecId;
use base_common_precompiles::UnstablePrecompiles;

let precompiles = UnstablePrecompiles::new_with_spec(UnstableSpecId::new(UnstableUpgrade::Azul));
let _active = precompiles.precompiles();
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
