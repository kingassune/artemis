use artemis_core::collectors::block_collector::NewBlock;
use ethers::types::{H160, U256};

/// Top-level events processed by the liquidation strategy.
#[derive(Debug, Clone)]
pub enum Event {
    /// A new block was mined.
    NewBlock(NewBlock),
    /// A price oracle update for a tracked asset.
    PriceUpdate(PriceUpdate),
}

/// A price oracle update for a single asset.
#[derive(Debug, Clone)]
pub struct PriceUpdate {
    /// The ERC-20 asset address.
    pub asset: H160,
    /// Price in USD scaled by 1e18 (e.g. $1.00 = 1_000_000_000_000_000_000).
    pub price_usd_e18: U256,
}

/// Top-level actions produced by the liquidation strategy.
#[derive(Debug, Clone)]
pub enum Action {
    /// Submit a liquidation for an undercollateralised position.
    LiquidatePosition(LiquidatePosition),
}

/// All information needed to call the lending protocol's liquidation function.
#[derive(Debug, Clone)]
pub struct LiquidatePosition {
    /// The borrower whose position should be liquidated.
    pub borrower: H160,
    /// The asset posted as collateral.
    pub collateral_asset: H160,
    /// The asset that was borrowed.
    pub debt_asset: H160,
    /// Amount of collateral to seize, in the collateral asset's native decimals (1e18 = 1 token).
    pub collateral_amount: U256,
    /// Amount of debt to repay, in the debt asset's native decimals (1e18 = 1 token).
    pub debt_amount: U256,
}

/// A borrower position tracked by the strategy.
#[derive(Debug, Clone)]
pub struct Position {
    /// The borrower's address.
    pub borrower: H160,
    /// The asset posted as collateral.
    pub collateral_asset: H160,
    /// Collateral balance in the asset's native decimals (1e18 = 1 token).
    pub collateral_amount: U256,
    /// The asset that was borrowed.
    pub debt_asset: H160,
    /// Debt balance in the asset's native decimals (1e18 = 1 token).
    pub debt_amount: U256,
    /// Minimum collateralisation ratio, expressed as an integer percentage.
    /// E.g. 150 means the position must hold at least 1.5× collateral vs. debt
    /// value before it becomes eligible for liquidation.
    pub liquidation_threshold: U256,
}

/// Static configuration for the liquidation strategy.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Address that will receive liquidation rewards.
    pub liquidator_address: H160,
}
