//! CLI argument parsing for the load-test binary.

use std::path::PathBuf;

use alloy_signer_local::PrivateKeySigner;
use base_load_tests::{LoadTest, LoadTestOptions, Rescue, RescueOptions};
use clap::{ArgGroup, Args, Parser, Subcommand};
use indicatif::MultiProgress;
use tracing_indicatif::{IndicatifWriter, writer::Stderr};
use tracing_subscriber::{
    EnvFilter, filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt,
};
use url::Url;

/// Load-test binary CLI.
#[derive(Debug, Parser)]
#[command(
    author,
    version = env!("CARGO_PKG_VERSION"),
    about = "Base network load test runner",
    long_about = None
)]
pub struct Cli {
    /// Default load-test arguments.
    #[command(flatten)]
    pub load: LoadArgs,

    /// Optional subcommand.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// CLI arguments for the default load-test command.
#[derive(Clone, Debug, Args)]
pub struct LoadArgs {
    /// YAML config file to run.
    #[arg(value_name = "CONFIG")]
    pub config: Option<PathBuf>,

    /// Run indefinitely until interrupted.
    #[arg(long)]
    pub continuous: bool,

    /// Drain accounts from the config without running a load test.
    #[arg(long)]
    pub drain_only: bool,
}

/// Load-test subcommands.
#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    /// Rescue stranded funds by deriving accounts from a seed or mnemonic.
    Rescue(RescueArgs),
}

/// CLI arguments for the rescue subcommand.
#[derive(Clone, Debug, Args)]
#[command(group(ArgGroup::new("derivation").required(true).args(["seed", "mnemonic"])))]
pub struct RescueArgs {
    /// RPC endpoint.
    #[arg(long = "rpc-url", alias = "rpc")]
    pub rpc_url: Url,

    /// Seed used for account generation.
    #[arg(long)]
    pub seed: Option<u64>,

    /// Mnemonic used for account generation.
    #[arg(long)]
    pub mnemonic: Option<String>,

    /// Number of accounts to scan.
    #[arg(long = "count", default_value_t = RescueOptions::DEFAULT_SCAN_COUNT)]
    pub scan_count: usize,

    /// Starting account offset.
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Private key of the funder account.
    #[arg(long = "funder-key", env = "FUNDER_KEY")]
    pub funder_key: PrivateKeySigner,
}

impl Cli {
    /// Runs the load-test CLI.
    pub async fn run(self) -> eyre::Result<()> {
        match self.command {
            Some(Commands::Rescue(args)) => {
                Self::init_tracing()?;
                Rescue::run(args.into()).await
            }
            None => {
                let mp = Self::init_progress_tracing()?;
                LoadTest::run_with_progress(self.load.into(), &mp).await
            }
        }
    }

    /// Initialises standard tracing output for non-interactive commands.
    pub fn init_tracing() -> eyre::Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
            .try_init()
            .map_err(|e| eyre::eyre!("failed to initialize tracing: {e}"))?;

        Ok(())
    }

    /// Initialises progress-bar-aware tracing for the default load-test command.
    pub fn init_progress_tracing() -> eyre::Result<MultiProgress> {
        let mp = MultiProgress::new();
        let writer: IndicatifWriter<Stderr> = IndicatifWriter::new(mp.clone());
        let filter =
            EnvFilter::builder().with_default_directive(LevelFilter::WARN.into()).from_env_lossy();

        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(true))
            .with(filter)
            .try_init()
            .map_err(|e| eyre::eyre!("failed to initialize tracing: {e}"))?;

        Ok(mp)
    }
}

impl From<LoadArgs> for LoadTestOptions {
    fn from(args: LoadArgs) -> Self {
        Self { config_path: args.config, continuous: args.continuous, drain_only: args.drain_only }
    }
}

impl From<RescueArgs> for RescueOptions {
    fn from(args: RescueArgs) -> Self {
        Self {
            rpc_url: args.rpc_url,
            seed: args.seed,
            scan_count: args.scan_count,
            offset: args.offset,
            funder_key: args.funder_key,
            mnemonic: args.mnemonic,
        }
    }
}
