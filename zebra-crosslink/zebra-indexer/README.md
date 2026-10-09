# Zebra Crosslink Explorer Indexer

This crate provides the response models, canonical-state adapters, and durable
RocksDB indexes used by Zebra Crosslink's explorer JSON-RPC methods.

The explorer is opt-in. Build `zebrad` with the `indexer` feature and configure
the normal JSON-RPC listener:

```toml
[state]
# This must be a fresh cache directory so explorer indexes are built from genesis.
cache_dir = "/absolute/path/to/fresh-crosslink-indexer-cache"

[rpc]
listen_addr = "127.0.0.1:8232"

# Convenient for local development only. Keep authentication enabled when the
# endpoint is reachable by anyone else.
enable_cookie_auth = false
```

Then start the node from the `zebra-crosslink` workspace:

```sh
cargo run --release -p zebrad --features indexer -- -c /path/to/zebrad-indexer.toml start
```

`rpc.indexer_listen_addr` is Zebra's separate gRPC indexer service; these
explorer methods use `rpc.listen_addr`.

The feature adds these JSON-RPC methods:

- Blocks: `getblocks`, `getblockdetails`, `gettopminers`, `getminerinfo`
- Transactions: `gettransactions`, `getmempooltransactions`, `gettransactiondetails`
- Addresses: `getaddresssummary`, `getaddresstransactions`, `getaddressutxospage`
- Explorer: `getindexerstatus`, `getnetworkstats`, `getexplorerchartdata`,
  `getexplorertopbalances`
- Crosslink stake: `getcrosslinkminerstake`, `getcrosslinkfinalizerstakesources`,
  `getcrosslinkstakehistory`

For example:

```sh
curl -sS http://127.0.0.1:8232 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getblocks","params":[{"limit":20}]}'
```

The all-time miner ranking is maintained incrementally. Each request reads only the requested
ranking slice and the latest block for each returned miner to derive pool attribution:

```sh
curl -sS http://127.0.0.1:8232 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"gettopminers","params":[{"limit":30,"direction":"next"}]}'
```

Pass `pagination.next_cursor` back as `cursor` with `direction: "next"`, or
`pagination.prev_cursor` with `direction: "prev"`. Cursors are bound to the finalized chain tip;
if that ranking generation changes, restart from the first page.

The existing `getcrosslinkminerstake` and `getcrosslinkfinalizerstakesources` methods
return all current transparent funding sources, including addresses that have never mined.
Each item includes `address` and `is_miner`. Non-miners have
`blocks_mined: "0"` and `pool: "Unknown"`.
`pool` is a mining-pool label; `finalizer_public_key` identifies the stake target.
Finalizer ranking entries expose the highest-stake transparent source as
`primary_stake_address` and the total number of transparent sources as
`transparent_address_count`.

To check the current active stake of one address in a finalizer, use the existing
finalizer sources method with both filters:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "getcrosslinkfinalizerstakesources",
  "params": [{"public_key": "<finalizer public key in hex>", "address": "<transparent address>", "limit": 30}]
}
```

`getcrosslinkminerstake` also accepts optional `address` and `is_miner` filters.
Use `is_miner: true` for miner-only allocations and `is_miner: false` for
transparent sources that have never mined. These filters apply to `items`,
`summary.pair_count`, and `pagination.total`; coverage totals and percentages remain
scoped to the whole network or the selected finalizer. `summary.sources.miners` and
`summary.sources.others` retain their separate totals. `transparent_attributed_stake_zat`
includes both, and `unattributed_stake_zat` excludes both.

Stake attribution uses the original transaction's representative primary value source,
as in the miner attribution model. A transaction with several funding sources is not
split among its input addresses. Shielded sources expose their category and amount,
not a funding address. Retargeting keeps the original source address; beginning
unbonding removes the pair from current active stake.

For historical actions, use `getcrosslinkstakehistory` with `address` and
`finalizer_public_key`. This includes actions entering or leaving the finalizer,
including retarget, begin-unbonding, and withdrawal, even after active stake is zero.
The source address refers to the original bond funding, rather than later transaction
fee inputs.

Explorer schema 4 databases are upgraded on writable startup by backfilling the
transparent stake rankings from existing pair metadata. No blockchain resync is
required for this upgrade. Start the writable node before opening read-only replicas;
older explorer schemas retain their existing resync requirement. Stake ranking cursors
from earlier builds or a different `address`/`is_miner` filter must be replaced by
requesting the first page again.
