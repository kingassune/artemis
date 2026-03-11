mod strategy;
mod types;

use std::sync::Arc;

use anyhow::Result;
use artemis_core::{
    collectors::block_collector::BlockCollector,
    engine::Engine,
    executors::mempool_executor::MempoolExecutor,
    types::{CollectorMap, ExecutorMap},
};
use clap::Parser;
use ethers::{
    prelude::MiddlewareBuilder,
    providers::{Provider, Ws},
    signers::{LocalWallet, Signer},
    types::Address,
};
use strategy::SimpleDexArb;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
use types::{Action, ArbConfig, Event};

/// CLI arguments for the simple-dex-arb bot.
#[derive(Parser, Debug)]
pub struct Args {
    /// Ethereum WebSocket node endpoint.
    #[arg(long)]
    pub wss: String,

    /// Private key used to sign and submit transactions.
    #[arg(long)]
    pub private_key: String,

    /// Address of the first Uniswap V2-style pool (pool A).
    #[arg(long)]
    pub pool_a: Address,

    /// Address of the second Uniswap V2-style pool (pool B).
    #[arg(long)]
    pub pool_b: Address,

    /// Minimum profit in wei before the bot submits an arbitrage transaction.
    #[arg(long, default_value = "0")]
    pub min_profit_wei: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up structured tracing to stdout.
    let filter = filter::Targets::new()
        .with_target("simple_dex_arb", Level::INFO)
        .with_target("artemis_core", Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let args = Args::parse();

    // Connect to the Ethereum node and attach a signer.
    let ws = Ws::connect(args.wss).await?;
    let provider = Provider::new(ws);

    let wallet: LocalWallet = args.private_key.parse()?;
    let address = wallet.address();

    let provider = Arc::new(provider.nonce_manager(address).with_signer(wallet));

    // Build the Artemis engine.
    let mut engine: Engine<Event, Action> = Engine::default();

    // --- Collector ---
    // BlockCollector drives the strategy on every new block.
    // In production, add a LogCollector watching Uniswap V2 Sync events for
    // both pools, mapping logs to PoolReservesUpdate events.
    let block_collector = Box::new(BlockCollector::new(provider.clone()));
    let block_collector = CollectorMap::new(block_collector, Event::NewBlock);
    engine.add_collector(Box::new(block_collector));

    // --- Strategy ---
    let config = ArbConfig {
        pool_a: args.pool_a,
        pool_b: args.pool_b,
        min_profit_wei: args.min_profit_wei.into(),
    };
    engine.add_strategy(Box::new(SimpleDexArb::new(config)));

    // --- Executor ---
    let mempool_executor = Box::new(MempoolExecutor::new(provider.clone()));
    let mempool_executor = ExecutorMap::new(mempool_executor, |action| match action {
        Action::SubmitTx(tx) => Some(tx),
    });
    engine.add_executor(Box::new(mempool_executor));

    // Run the engine until all tasks complete.
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("task finished: {:?}", res);
        }
    }

    Ok(())
}
