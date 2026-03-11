use anyhow::Result;
use async_trait::async_trait;
use artemis_core::{executors::mempool_executor::SubmitTxToMempool, types::Strategy};
use ethers::types::{TransactionRequest, U256};
use tracing::info;

use crate::types::{Action, ArbConfig, Event};

pub struct SimpleDexArb {
    config: ArbConfig,
    /// Cached reserves for pool A as (reserve0, reserve1).
    reserves_a: Option<(U256, U256)>,
    /// Cached reserves for pool B as (reserve0, reserve1).
    reserves_b: Option<(U256, U256)>,
}

impl SimpleDexArb {
    pub fn new(config: ArbConfig) -> Self {
        Self {
            config,
            reserves_a: None,
            reserves_b: None,
        }
    }

    /// Compute the output amount for a Uniswap V2 swap with a 0.3% fee.
    ///
    /// Returns `None` on overflow or if reserves are zero.
    fn get_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> Option<U256> {
        if reserve_in.is_zero() || reserve_out.is_zero() || amount_in.is_zero() {
            return None;
        }
        let amount_in_with_fee = amount_in.checked_mul(U256::from(997))?;
        let numerator = amount_in_with_fee.checked_mul(reserve_out)?;
        let denominator = reserve_in
            .checked_mul(U256::from(1000))?
            .checked_add(amount_in_with_fee)?;
        numerator.checked_div(denominator)
    }

    /// Check whether a cross-pool arbitrage is profitable.
    ///
    /// Returns `Some((amount_in, profit))` when a trade of `amount_in` token1
    /// through the cheaper pool and back through the more expensive pool
    /// yields a positive profit after fees.  Returns `None` otherwise.
    fn compute_arb(&self) -> Option<(U256, U256)> {
        let (ra0, ra1) = self.reserves_a?;
        let (rb0, rb1) = self.reserves_b?;

        if ra0.is_zero() || rb0.is_zero() {
            return None;
        }

        // Compare prices (token1 per token0) using cross-multiplication to
        // avoid division and stay in integer arithmetic.
        // price_a = ra1 / ra0   vs   price_b = rb1 / rb0
        // price_a > price_b  ⟺  ra1 * rb0 > rb1 * ra0
        let price_a_scaled = ra1.checked_mul(rb0)?;
        let price_b_scaled = rb1.checked_mul(ra0)?;

        // Determine which pool is cheaper for token0 (i.e., has lower token1
        // per token0 price) so we buy token0 there and sell it in the other.
        let (buy_r0, buy_r1, sell_r0, sell_r1) = if price_a_scaled < price_b_scaled {
            // Pool A is cheaper for token0 – buy token0 in A, sell in B.
            (ra0, ra1, rb0, rb1)
        } else if price_b_scaled < price_a_scaled {
            // Pool B is cheaper for token0 – buy token0 in B, sell in A.
            (rb0, rb1, ra0, ra1)
        } else {
            // Prices are equal – no arb.
            return None;
        };

        // Use 1 % of the cheaper pool's token1 reserves as the trial input.
        let amount_in = buy_r1.checked_div(U256::from(100))?;
        if amount_in.is_zero() {
            return None;
        }

        // Step 1: spend `amount_in` token1 in the buy-pool to receive token0.
        let token0_received = Self::get_amount_out(amount_in, buy_r1, buy_r0)?;

        // Step 2: sell the received token0 in the sell-pool to receive token1.
        let token1_received = Self::get_amount_out(token0_received, sell_r0, sell_r1)?;

        // Profit is the surplus token1 after paying `amount_in`.
        if token1_received > amount_in {
            let profit = token1_received - amount_in;
            Some((amount_in, profit))
        } else {
            None
        }
    }
}

#[async_trait]
impl Strategy<Event, Action> for SimpleDexArb {
    async fn sync_state(&mut self) -> Result<()> {
        Ok(())
    }

    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::NewBlock(block) => {
                info!(block_number = %block.number, "New block");
                vec![]
            }
            Event::PoolReservesUpdate(update) => {
                if update.pool_address == self.config.pool_a {
                    info!(
                        pool = ?update.pool_address,
                        reserve0 = %update.reserve0,
                        reserve1 = %update.reserve1,
                        "Pool A reserves updated"
                    );
                    self.reserves_a = Some((update.reserve0, update.reserve1));
                } else if update.pool_address == self.config.pool_b {
                    info!(
                        pool = ?update.pool_address,
                        reserve0 = %update.reserve0,
                        reserve1 = %update.reserve1,
                        "Pool B reserves updated"
                    );
                    self.reserves_b = Some((update.reserve0, update.reserve1));
                }

                if let Some((amount_in, profit)) = self.compute_arb() {
                    if profit >= self.config.min_profit_wei {
                        info!(%amount_in, %profit, "Arbitrage opportunity found – submitting tx");

                        // TODO: replace with a real call to an on-chain arb contract
                        // (set `to`, `data`, and `value` fields).  This bare
                        // placeholder is intentional for the example and will
                        // revert if actually submitted to the network.
                        let tx: ethers::types::transaction::eip2718::TypedTransaction =
                            TransactionRequest::new().into();

                        return vec![Action::SubmitTx(SubmitTxToMempool {
                            tx,
                            gas_bid_info: None,
                        })];
                    }
                }

                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArbConfig, PoolReservesUpdate};
    use ethers::types::{H160, U256};

    fn make_config(pool_a: H160, pool_b: H160) -> ArbConfig {
        ArbConfig {
            pool_a,
            pool_b,
            min_profit_wei: U256::zero(),
        }
    }

    fn pool_update(addr: H160, reserve0: U256, reserve1: U256) -> Event {
        Event::PoolReservesUpdate(PoolReservesUpdate {
            pool_address: addr,
            reserve0,
            reserve1,
        })
    }

    /// Helper: 10^18
    fn e18(n: u64) -> U256 {
        U256::from(n) * U256::exp10(18)
    }

    /// Helper: 10^6
    fn e6(n: u64) -> U256 {
        U256::from(n) * U256::exp10(6)
    }

    #[tokio::test]
    async fn test_arb_detected_when_prices_differ() {
        let pool_a = H160::from_low_u64_be(1);
        let pool_b = H160::from_low_u64_be(2);
        let mut strategy = SimpleDexArb::new(make_config(pool_a, pool_b));

        // Pool A: 100 ETH / 200 000 USDC  →  price = 2 000 USDC/ETH
        let actions = strategy
            .process_event(pool_update(pool_a, e18(100), e6(200_000)))
            .await;
        assert!(actions.is_empty(), "no arb until both pools are known");

        // Pool B: 100 ETH / 180 000 USDC  →  price = 1 800 USDC/ETH (cheaper)
        let actions = strategy
            .process_event(pool_update(pool_b, e18(100), e6(180_000)))
            .await;

        assert_eq!(actions.len(), 1, "should produce exactly one arb action");
        assert!(
            matches!(actions[0], Action::SubmitTx(_)),
            "action should be SubmitTx"
        );
    }

    #[tokio::test]
    async fn test_no_arb_when_prices_equal() {
        let pool_a = H160::from_low_u64_be(1);
        let pool_b = H160::from_low_u64_be(2);
        let mut strategy = SimpleDexArb::new(make_config(pool_a, pool_b));

        // Both pools at the same 2 000 USDC/ETH price.
        strategy
            .process_event(pool_update(pool_a, e18(100), e6(200_000)))
            .await;
        let actions = strategy
            .process_event(pool_update(pool_b, e18(100), e6(200_000)))
            .await;

        assert!(actions.is_empty(), "equal prices should yield no arb");
    }

    #[tokio::test]
    async fn test_no_arb_with_single_pool() {
        let pool_a = H160::from_low_u64_be(1);
        let pool_b = H160::from_low_u64_be(2);
        let mut strategy = SimpleDexArb::new(make_config(pool_a, pool_b));

        // Only pool A is known – pool B reserves are still None.
        let actions = strategy
            .process_event(pool_update(pool_a, e18(100), e6(200_000)))
            .await;

        assert!(
            actions.is_empty(),
            "should not arb with only one pool's reserves known"
        );
    }
}
