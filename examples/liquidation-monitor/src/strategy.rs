use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use artemis_core::types::Strategy;
use ethers::types::{H160, U256};
use tracing::info;

use crate::types::{Action, Event, LiquidatePosition, MonitorConfig, Position};

/// Monitors a set of lending positions and emits liquidation actions whenever a
/// position's collateralisation ratio drops below its liquidation threshold.
pub struct LiquidationMonitor {
    config: MonitorConfig,
    /// All positions being tracked.
    positions: Vec<Position>,
    /// Latest known price for each asset, in USD scaled by 1e18.
    prices: HashMap<H160, U256>,
}

impl LiquidationMonitor {
    pub fn new(config: MonitorConfig) -> Self {
        Self {
            config,
            positions: Vec::new(),
            prices: HashMap::new(),
        }
    }

    /// Populate the initial set of positions (builder pattern).
    pub fn with_positions(mut self, positions: Vec<Position>) -> Self {
        self.positions = positions;
        self
    }

    /// Add a single position to the watch-list.
    pub fn add_position(&mut self, position: Position) {
        self.positions.push(position);
    }

    /// Scan every tracked position and return liquidation actions for any that
    /// are currently undercollateralised.
    fn check_liquidations(&self) -> Vec<Action> {
        self.positions
            .iter()
            .filter(|p| self.is_liquidatable(p))
            .map(|p| {
                info!(
                    borrower = ?p.borrower,
                    collateral_asset = ?p.collateral_asset,
                    debt_asset = ?p.debt_asset,
                    "Undercollateralised position found – emitting liquidation action"
                );
                Action::LiquidatePosition(LiquidatePosition {
                    borrower: p.borrower,
                    collateral_asset: p.collateral_asset,
                    debt_asset: p.debt_asset,
                    collateral_amount: p.collateral_amount,
                    debt_amount: p.debt_amount,
                })
            })
            .collect()
    }

    /// Return `true` when the position's collateral value (in USD) multiplied
    /// by 100 is less than the debt value multiplied by the liquidation
    /// threshold, i.e. when:
    ///
    ///   collateral_value_usd * 100 < debt_value_usd * liquidation_threshold
    ///
    /// Both values are computed as `amount * price_usd_e18 / 1e18` so that the
    /// result is still expressed in USD × 1e18, keeping the comparison exact
    /// without any floating-point arithmetic.
    ///
    /// Returns `false` when either price is unknown or arithmetic overflows.
    fn is_liquidatable(&self, position: &Position) -> bool {
        let collateral_price = match self.prices.get(&position.collateral_asset) {
            Some(p) => *p,
            None => return false,
        };
        let debt_price = match self.prices.get(&position.debt_asset) {
            Some(p) => *p,
            None => return false,
        };

        let collateral_value = match Self::compute_value_usd_e18(position.collateral_amount, collateral_price) {
            Some(v) => v,
            None => return false,
        };
        let debt_value = match Self::compute_value_usd_e18(position.debt_amount, debt_price) {
            Some(v) => v,
            None => return false,
        };

        // collateral_value * 100 < debt_value * liquidation_threshold
        let lhs = match collateral_value.checked_mul(U256::from(100)) {
            Some(v) => v,
            None => return false,
        };
        let rhs = match debt_value.checked_mul(position.liquidation_threshold) {
            Some(v) => v,
            None => return false,
        };

        lhs < rhs
    }

    /// Compute the USD value of `amount` tokens whose price is `price_usd_e18`.
    ///
    /// Both `amount` and `price_usd_e18` are scaled by 1e18, so the result is
    /// divided by 1e18 to keep the value in USD × 1e18.
    ///
    /// Returns `None` on overflow or divide-by-zero.
    fn compute_value_usd_e18(amount: U256, price_usd_e18: U256) -> Option<U256> {
        amount
            .checked_mul(price_usd_e18)?
            .checked_div(U256::exp10(18))
    }
}

#[async_trait]
impl Strategy<Event, Action> for LiquidationMonitor {
    async fn sync_state(&mut self) -> Result<()> {
        info!(
            liquidator = ?self.config.liquidator_address,
            positions = self.positions.len(),
            "LiquidationMonitor synced – watching {} position(s)",
            self.positions.len()
        );
        Ok(())
    }

    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::NewBlock(block) => {
                info!(block_number = %block.number, "New block – re-checking liquidations");
                self.check_liquidations()
            }
            Event::PriceUpdate(update) => {
                info!(
                    asset = ?update.asset,
                    price_usd_e18 = %update.price_usd_e18,
                    "Price update received"
                );
                self.prices.insert(update.asset, update.price_usd_e18);
                self.check_liquidations()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MonitorConfig, PriceUpdate};
    use artemis_core::collectors::block_collector::NewBlock;
    use ethers::types::{H160, H256, U64, U256};

    fn make_config() -> MonitorConfig {
        MonitorConfig {
            liquidator_address: H160::from_low_u64_be(0xdead),
        }
    }

    /// 1e18 units of an asset worth `usd_cents` cents each (i.e. price_usd_e18
    /// = usd_cents * 1e16).  Handy for building round-number test positions.
    fn e18(n: u64) -> U256 {
        U256::from(n) * U256::exp10(18)
    }

    fn price_e18(dollars: u64) -> U256 {
        U256::from(dollars) * U256::exp10(18)
    }

    fn addr(n: u64) -> H160 {
        H160::from_low_u64_be(n)
    }

    // ── test 1: healthy position is NOT flagged ───────────────────────────────

    #[test]
    fn test_healthy_position_not_liquidatable() {
        // Collateral: 200 tokens at $1 = $200 value
        // Debt:       100 tokens at $1 = $100 value
        // Threshold:  150 %  →  need at least $150 collateral per $100 debt
        // $200 * 100 = 20 000 ≥ $100 * 150 = 15 000  →  healthy
        let collateral = addr(1);
        let debt = addr(2);

        let position = Position {
            borrower: addr(0xb0),
            collateral_asset: collateral,
            collateral_amount: e18(200),
            debt_asset: debt,
            debt_amount: e18(100),
            liquidation_threshold: U256::from(150),
        };

        let mut monitor = LiquidationMonitor::new(make_config());
        monitor.add_position(position);
        monitor.prices.insert(collateral, price_e18(1));
        monitor.prices.insert(debt, price_e18(1));

        assert!(!monitor.is_liquidatable(&monitor.positions[0].clone()));
    }

    // ── test 2: undercollateralised position IS flagged ───────────────────────

    #[test]
    fn test_undercollateralized_position_is_liquidatable() {
        // Collateral: 100 tokens at $1  = $100 value
        // Debt:        80 tokens at $2  = $160 value
        // Threshold:  150 %
        // $100 * 100 = 10 000 < $160 * 150 = 24 000  →  liquidatable
        let collateral = addr(1);
        let debt = addr(2);

        let position = Position {
            borrower: addr(0xb0),
            collateral_asset: collateral,
            collateral_amount: e18(100),
            debt_asset: debt,
            debt_amount: e18(80),
            liquidation_threshold: U256::from(150),
        };

        let mut monitor = LiquidationMonitor::new(make_config());
        monitor.add_position(position);
        monitor.prices.insert(collateral, price_e18(1));
        monitor.prices.insert(debt, price_e18(2));

        assert!(monitor.is_liquidatable(&monitor.positions[0].clone()));
    }

    // ── test 3: price drop triggers liquidation via process_event ─────────────

    #[tokio::test]
    async fn test_price_update_triggers_liquidation() {
        // Start healthy: 200 tokens collateral at $1 vs 100 tokens debt at $1
        let collateral = addr(1);
        let debt = addr(2);

        let position = Position {
            borrower: addr(0xb0),
            collateral_asset: collateral,
            collateral_amount: e18(200),
            debt_asset: debt,
            debt_amount: e18(100),
            liquidation_threshold: U256::from(150),
        };

        let mut monitor = LiquidationMonitor::new(make_config()).with_positions(vec![position]);
        // Seed initial prices so both are known.
        monitor.prices.insert(collateral, price_e18(1));
        monitor.prices.insert(debt, price_e18(1));

        // Collateral price drops to $0.50 → value = $100 < $100 * 150% = $150
        // $100 * 100 = 10 000 < $100 * 150 = 15 000  →  liquidatable
        let actions = monitor
            .process_event(Event::PriceUpdate(PriceUpdate {
                asset: collateral,
                // $0.50 = 5 * 1e17
                price_usd_e18: U256::from(5) * U256::exp10(17),
            }))
            .await;

        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], Action::LiquidatePosition(_)));
    }

    // ── test 4: NewBlock event also triggers liquidation check ────────────────

    #[tokio::test]
    async fn test_new_block_checks_liquidations() {
        let collateral = addr(1);
        let debt = addr(2);

        // Position is already liquidatable (same as test 2).
        let position = Position {
            borrower: addr(0xb0),
            collateral_asset: collateral,
            collateral_amount: e18(100),
            debt_asset: debt,
            debt_amount: e18(80),
            liquidation_threshold: U256::from(150),
        };

        let mut monitor = LiquidationMonitor::new(make_config()).with_positions(vec![position]);
        monitor.prices.insert(collateral, price_e18(1));
        monitor.prices.insert(debt, price_e18(2));

        let actions = monitor
            .process_event(Event::NewBlock(NewBlock {
                hash: H256::zero(),
                number: U64::from(42),
            }))
            .await;

        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], Action::LiquidatePosition(_)));
    }

    // ── test 5: no action when prices are unknown ─────────────────────────────

    #[tokio::test]
    async fn test_no_action_when_prices_unknown() {
        let position = Position {
            borrower: addr(0xb0),
            collateral_asset: addr(1),
            collateral_amount: e18(100),
            debt_asset: addr(2),
            debt_amount: e18(100),
            liquidation_threshold: U256::from(150),
        };

        // No prices inserted – both assets are unknown.
        let mut monitor = LiquidationMonitor::new(make_config()).with_positions(vec![position]);

        let actions = monitor
            .process_event(Event::NewBlock(NewBlock {
                hash: H256::zero(),
                number: U64::from(1),
            }))
            .await;

        assert!(actions.is_empty());
    }
}
