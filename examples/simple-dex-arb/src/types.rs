use artemis_core::{
    collectors::block_collector::NewBlock, executors::mempool_executor::SubmitTxToMempool,
};
use ethers::types::{H160, U256};

/// Top-level events processed by the strategy.
#[derive(Debug, Clone)]
pub enum Event {
    NewBlock(NewBlock),
    PoolReservesUpdate(PoolReservesUpdate),
}

/// A reserve update emitted when a Uniswap V2-style pool's Sync event fires.
#[derive(Debug, Clone)]
pub struct PoolReservesUpdate {
    pub pool_address: H160,
    pub reserve0: U256,
    pub reserve1: U256,
}

/// Top-level actions produced by the strategy.
#[derive(Debug, Clone)]
pub enum Action {
    SubmitTx(SubmitTxToMempool),
}

/// Static configuration for the arbitrage strategy.
#[derive(Debug, Clone)]
pub struct ArbConfig {
    /// Address of the first Uniswap V2-style pool.
    pub pool_a: H160,
    /// Address of the second Uniswap V2-style pool.
    pub pool_b: H160,
    /// Minimum profit in wei required before submitting a transaction.
    pub min_profit_wei: U256,
}
