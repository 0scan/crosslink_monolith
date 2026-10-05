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
