# `base-execution-cli`

CLI extensions for the Unstable execution node.

## Overview

Provides the command-line interface for the Unstable execution node. Wraps argument parsing with
Unstable-specific chain spec resolution via `UnstableChainSpecParser`, and exposes a `Cli` type that
drives node startup from parsed arguments.

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
base-execution-cli = { workspace = true }
```

```rust,ignore
use base_execution_cli::Cli;

fn main() {
    let cli = Cli::parse_args();
    cli.run(|builder, args| async move {
        // launch node
    });
}
```

## License

Licensed under the [MIT License](https://github.com/base/base/blob/main/LICENSE).
