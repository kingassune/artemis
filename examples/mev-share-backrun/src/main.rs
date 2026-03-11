mod strategy;
mod types;

use anyhow::Result;
use artemis_core::{
    collectors::mevshare_collector::MevShareCollector,
    engine::Engine,
    executors::mev_share_executor::MevshareExecutor,
    types::{CollectorMap, ExecutorMap},
};
use clap::Parser;
use ethers::signers::LocalWallet;
use ethers::types::Address;
use strategy::MevShareBackrun;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
use types::{Action, BackrunConfig, Event};

/// CLI arguments for the MEV-Share backrun bot.
#[derive(Parser, Debug)]
#[clap(about = "Simple MEV-Share backrun example using the Artemis framework")]
pub struct Args {
    /// Private key used to sign Flashbots authentication headers (hex, no 0x prefix).
    #[arg(long)]
    pub flashbots_signer: String,

    /// MEV-Share SSE endpoint URL.
    #[arg(long, default_value = "https://mev-share.flashbots.net")]
    pub mev_share_url: String,

    /// Address of the contract that will execute the backrun.
    #[arg(long)]
    pub executor_address: Address,
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = filter::Targets::new()
        .with_target("mev_share_backrun", Level::INFO)
        .with_target("artemis_core", Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let args = Args::parse();

    let fb_signer: LocalWallet = args.flashbots_signer.parse()?;

    let config = BackrunConfig {
        executor_address: args.executor_address,
        mev_share_url: args.mev_share_url.clone(),
    };

    // Build engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // Collector: stream MEV-Share SSE events and wrap them in our Event enum.
    let mevshare_collector = Box::new(MevShareCollector::new(args.mev_share_url));
    let mevshare_collector = CollectorMap::new(mevshare_collector, Event::MevShareEvent);
    engine.add_collector(Box::new(mevshare_collector));

    // Strategy: backrun any event that exposes logs.
    let strategy = MevShareBackrun::new(config);
    engine.add_strategy(Box::new(strategy));

    // Executor: forward SubmitBundle actions to the MEV-Share relay.
    let mev_share_executor = Box::new(MevshareExecutor::new(fb_signer));
    let mev_share_executor = ExecutorMap::new(mev_share_executor, |action| match action {
        Action::SubmitBundle(bundle) => Some(bundle),
    });
    engine.add_executor(Box::new(mev_share_executor));

    // Run engine.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("task result: {:?}", res);
        }
    }

    Ok(())
}
