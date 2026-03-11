# liquidation-monitor

An example MEV bot built with the [Artemis](../../README.md) framework that monitors
DeFi lending positions and submits liquidation transactions whenever a borrower
becomes undercollateralised.

## What this demonstrates

| Artemis concept | Implementation |
|---|---|
| **Collector** | `BlockCollector` – fires on every new Ethereum block |
| **Strategy** | `LiquidationMonitor` – tracks positions and checks health factors |
| **Executor** | `MempoolExecutor` via `ExecutorMap` – submits liquidation txs |
| **Type mapping** | `CollectorMap` wraps block events into the local `Event` enum |

The data flows through the engine like this:

```
BlockCollector ──(NewBlock)──▶ LiquidationMonitor ──(LiquidatePosition)──▶ MempoolExecutor
                                        ▲
                        PriceUpdate events (injected externally
                        or via an oracle collector in production)
```

## Key concepts

### Health factor and liquidation threshold

Each position has a **liquidation threshold** expressed as an integer
percentage (e.g. `150` means 150 %).  A position is **liquidatable** when:

```
collateral_value_usd × 100  <  debt_value_usd × liquidation_threshold
```

where values are computed in integer arithmetic as:

```
value_usd = token_amount × price_usd_e18 / 1e18
```

`price_usd_e18` is the price in USD scaled by 10¹⁸ so that `$1.00 = 1_000_000_000_000_000_000`.

### Example

| Field | Value |
|---|---|
| Collateral | 100 WETH × $1 500/WETH = $150 000 |
| Debt | 120 000 USDC × $1/USDC = $120 000 |
| Threshold | 150 % |
| Check | $150 000 × 100 = 15 000 000 ≥ $120 000 × 150 = 18 000 000 → **healthy** |

If the WETH price drops to $1 000:

| Field | Value |
|---|---|
| Collateral | 100 WETH × $1 000/WETH = $100 000 |
| Check | $100 000 × 100 = 10 000 000 **<** $120 000 × 150 = 18 000 000 → **liquidatable** |

## How to run

```bash
cargo run --bin liquidation-monitor -- \
  --wss wss://mainnet.infura.io/ws/v3/<YOUR_KEY> \
  --private-key <HEX_PRIVATE_KEY> \
  --liquidator-address <YOUR_ADDRESS>
```

> **Note:** The demo positions are hardcoded placeholders.  The bot will log
> liquidation opportunities but the placeholder transaction will revert on-chain
> because it does not encode real liquidation calldata.  See the production
> notes below.

## How to test

```bash
cargo test -p liquidation-monitor
```

The unit tests cover:

1. A healthy position is **not** flagged for liquidation.
2. An undercollateralised position **is** flagged.
3. A `PriceUpdate` event that drops collateral value below the threshold
   triggers a `LiquidatePosition` action.
4. A `NewBlock` event re-checks all positions and emits actions for those
   already liquidatable.
5. No action is emitted when asset prices are unknown.

## Production notes

To turn this into a production-ready liquidation bot:

1. **Position discovery** – add a `LogCollector` that listens to the lending
   protocol's `Borrow`, `Repay`, and `Liquidation` events so the strategy's
   position list stays in sync with on-chain state automatically.

2. **Price oracle** – add a collector that subscribes to Chainlink price-feed
   updates (or fetches prices from an on-chain oracle on each block) and emits
   `Event::PriceUpdate` events.

3. **Liquidation calldata** – replace the placeholder `TransactionRequest` in
   `main.rs` with a real call to the lending protocol's `liquidationCall`
   function, encoded via an `ethers` `Contract` binding (use `abigen!`).

4. **Gas bidding** – populate `SubmitTxToMempool::gas_bid_info` to bid a
   fraction of the expected liquidation bonus as the gas tip, ensuring
   profitability after fees.
