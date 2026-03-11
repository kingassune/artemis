# simple-dex-arb

A minimal example of a cross-DEX arbitrage bot built with the
[Artemis](https://github.com/paradigmxyz/artemis) MEV framework.

It monitors two Uniswap V2-style AMM pools for price discrepancies and
submits an arbitrage transaction to the public mempool whenever a profitable
trade is detected.

---

## What this demonstrates

| Concept | Where |
|---|---|
| `Collector<E>` – produce events | `BlockCollector` in `main.rs` |
| `Strategy<E, A>` – react to events | `SimpleDexArb` in `strategy.rs` |
| `Executor<A>` – act on decisions | `MempoolExecutor` in `main.rs` |
| `CollectorMap` / `ExecutorMap` | type adapters in `main.rs` |
| CFMM price formula | `get_amount_out` in `strategy.rs` |

### Collector → Strategy → Executor flow

```
BlockCollector ──(NewBlock)──► SimpleDexArb ──(SubmitTx)──► MempoolExecutor
LogCollector   ──(PoolReservesUpdate)──►  ╝
```

1. **BlockCollector** emits a `NewBlock` event on every Ethereum block.
2. A `LogCollector` (not wired in this example – see the comment in
   `main.rs`) would emit a `PoolReservesUpdate` event each time either pool
   emits a Uniswap V2 `Sync(uint112 reserve0, uint112 reserve1)` log.
3. `SimpleDexArb` updates its cached reserves on each `PoolReservesUpdate`
   and calls `compute_arb()` to decide whether to act.
4. When a profitable arb is found the strategy returns a `SubmitTx` action,
   which `MempoolExecutor` sends to the network.

---

## Key concepts

### CFMM constant-product formula (0.3 % fee)

```
amount_out = (amount_in × 997 × reserve_out)
             / (reserve_in × 1000 + amount_in × 997)
```

All arithmetic uses Rust's `U256` with `checked_mul` / `checked_div` so
overflows produce `None` rather than panicking.

### Price-discrepancy detection

The strategy compares prices with cross-multiplication to avoid integer
division:

```
price_a > price_b  ⟺  reserve1_a × reserve0_b > reserve1_b × reserve0_a
```

When a discrepancy exists it simulates buying token0 in the cheaper pool and
selling it in the more expensive pool using 1 % of the cheaper pool's token1
reserves as a trial input size.

### Atomic execution

In a production system the `TransactionRequest` placeholder would be replaced
with a call to an on-chain arb contract that executes both swaps atomically in
a single transaction, ensuring the bot either profits or reverts.

---

## Running

```bash
cargo run -p simple-dex-arb -- \
  --wss        wss://mainnet.infura.io/ws/v3/<YOUR_KEY> \
  --private-key <0xYOUR_PRIVATE_KEY> \
  --pool-a     0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc \
  --pool-b     0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852 \
  --min-profit-wei 1000000000000000
```

## Running the tests

```bash
cargo test -p simple-dex-arb
```

The unit tests in `strategy.rs` cover three scenarios:

| Test | What it verifies |
|---|---|
| `test_arb_detected_when_prices_differ` | A `SubmitTx` action is returned when pool prices diverge |
| `test_no_arb_when_prices_equal` | No action when both pools have the same price |
| `test_no_arb_with_single_pool` | No action when only one pool's reserves are known yet |
