# mev-share-backrun

A minimal, self-contained example of an **MEV-Share backrun bot** built with
the [Artemis](https://github.com/paradigmxyz/artemis) MEV framework.

## What this demonstrates

* The **Collector → Strategy → Executor** pipeline of Artemis.
* How to consume the [MEV-Share](https://docs.flashbots.net/flashbots-mev-share/overview)
  SSE stream with `MevShareCollector`.
* How to construct and submit a backrun bundle via `MevshareExecutor`.
* How to keep a strategy simple and unit-testable without a live Ethereum node.

## How MEV-Share backruns work

MEV-Share is a protocol that lets searchers receive hints about pending
transactions. When a transaction emits logs that indicate an interesting
on-chain action (e.g. a large Uniswap swap), a searcher can submit a *bundle*
that contains:

1. The pending transaction (`BundleItem::Hash`), so the backrun lands
   immediately after it.
2. The searcher's own transaction (`BundleItem::Tx`), which extracts value
   created by the first transaction.

The bundle includes an *inclusion range* (a start block and an optional last
valid block) so the relay only forwards it while it is still relevant.

## Architecture

```
MevShareCollector  ──►  MevShareBackrun  ──►  MevshareExecutor
(SSE stream)            (strategy)             (Flashbots relay)
```

| Component | File | Responsibility |
|-----------|------|----------------|
| `MevShareCollector` | artemis-core | Streams MEV-Share SSE events |
| `MevShareBackrun` | `src/strategy.rs` | Decides whether to backrun; builds the bundle |
| `MevshareExecutor` | artemis-core | Signs & forwards the bundle to the relay |

### Why `current_block` is tracked separately

The strategy does not hold an Ethereum provider to stay lightweight and easily
testable. In a production bot you would update `current_block` by also wiring
up a `BlockCollector` and handling a `NewBlock` event variant; here the field
defaults to `1` and can be updated via `set_block()`.

### Placeholder transaction

The backrun transaction (`0x0200`) is intentionally a stub. To build a real
bot, replace `create_backrun_bundle`'s `BundleItem::Tx` entry with a proper
signed EIP-1559 transaction that implements your backrun logic.

## Running

```bash
cargo run --bin mev-share-backrun -- \
  --flashbots-signer <PRIVATE_KEY_HEX> \
  --executor-address <CONTRACT_ADDRESS>
```

Optional flag (defaults to the public endpoint):

```bash
  --mev-share-url https://mev-share.flashbots.net
```

## Testing

```bash
cargo test -p mev-share-backrun
```

The unit tests in `strategy.rs` exercise `create_backrun_bundle` directly,
avoiding the need for a live node or network connection.
