mod strategy;
mod types;

use std::sync::Arc;

use anyhow::Result;
use artemis_core::{
    collectors::block_collector::BlockCollector,
    engine::Engine,
    executors::mempool_executor::{MempoolExecutor, SubmitTxToMempool},
    types::{CollectorMap, ExecutorMap},
};
use clap::Parser;
use ethers::{
    prelude::MiddlewareBuilder,
    providers::{Provider, Ws},
    signers::{LocalWallet, Signer},
    types::{Address, TransactionRequest, U256},
};
use strategy::LiquidationMonitor;
use tracing::{info, Level};
use tracing_subscriber::{filter, prelude::*};
use types::{Action, Event, MonitorConfig, Position};

/// CLI arguments for the liquidation-monitor bot.
#[derive(Parser, Debug)]
pub struct Args {
    /// Ethereum WebSocket node endpoint.
    #[arg(long)]
    pub wss: String,

    /// Private key used to sign and submit liquidation transactions.
    #[arg(long)]
    pub private_key: String,

    /// Address that will receive liquidation rewards (your wallet or a
    /// dedicated liquidation contract).
    #[arg(long)]
    pub liquidator_address: Address,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up structured tracing to stdout.
    let filter = filter::Targets::new()
        .with_target("liquidation_monitor", Level::INFO)
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
    // BlockCollector drives the liquidation check on every new block.
    //
    // In production you would also add a position collector that watches the
    // lending protocol's Borrow / Repay / Liquidation events so that
    // `self.positions` stays in sync with on-chain state without needing a
    // full node scan on every block.
    let block_collector = Box::new(BlockCollector::new(provider.clone()));
    let block_collector = CollectorMap::new(block_collector, Event::NewBlock);
    engine.add_collector(Box::new(block_collector));

    // --- Strategy ---
    // Hardcoded demo positions.  In production, load these from on-chain data
    // (e.g., by replaying Borrow events from the lending protocol) or from an
    // off-chain database that is kept up to date by a position-indexing service.
    let demo_positions = vec![
        Position {
            borrower: "0x1111111111111111111111111111111111111111"
                .parse()
                .unwrap(),
            collateral_asset: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // WETH
                .parse()
                .unwrap(),
            collateral_amount: U256::from(1_u128) * U256::exp10(18), // 1 WETH
            debt_asset: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48" // USDC
                .parse()
                .unwrap(),
            debt_amount: U256::from(1500_u128) * U256::exp10(6), // 1 500 USDC (6 decimals)
            liquidation_threshold: U256::from(150),              // 150 %
        },
    ];

    let config = MonitorConfig {
        liquidator_address: args.liquidator_address,
    };
    let strategy = LiquidationMonitor::new(config).with_positions(demo_positions);
    engine.add_strategy(Box::new(strategy));

    // --- Executor ---
    // Map a LiquidatePosition action to a raw mempool transaction.
    //
    // In production, replace the placeholder TransactionRequest with a real
    // call to the lending protocol's `liquidationCall` function, encoding the
    // correct calldata (borrower, collateral asset, debt asset, debt amount,
    // receive-aToken flag, etc.) via an ethers Contract binding.
    let mempool_executor = Box::new(MempoolExecutor::new(provider.clone()));
    let mempool_executor = ExecutorMap::new(mempool_executor, |action| match action {
        Action::LiquidatePosition(liq) => Some(SubmitTxToMempool {
            tx: TransactionRequest::new().to(liq.borrower).into(),
            gas_bid_info: None,
        }),
    });
    engine.add_executor(Box::new(mempool_executor));

    // Run the engine until all tasks complete (runs indefinitely in practice).
    if let Ok(mut set) = engine.run().await {
        while let Some(res) = set.join_next().await {
            info!("task finished: {:?}", res);
        }
    }

    Ok(())
}
