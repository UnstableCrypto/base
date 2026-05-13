# `base-execution-payload-builder`

Payload builder for Unstable.

## Overview

Implements Unstable payload building and validation for the Unstable execution node. The
`UnstablePayloadBuilder` assembles new execution payloads from transaction pool contents and
`UnstablePayloadBuilderAttributes` received from the consensus layer. `UnstableExecutionPayloadValidator`
verifies
built payloads against consensus rules. Also provides data availability configuration via
`UnstableDAConfig` for fee calculation.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-execution-payload-builder = { workspace = true }
```

```rust,ignore
use base_execution_payload_builder::UnstablePayloadBuilder;

let builder = UnstablePayloadBuilder::new(evm_config, payload_validator);
let payload = builder.build_payload(attrs, best_payload)?;
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
