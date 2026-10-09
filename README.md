# Important Disclaimer About This Repository

This repository exists to prove ideas, not to demonstrate production engineering. It is a prototype designed to let us move quickly and validate the Crosslink design.

It is **not** production-ready code, and it is **not** what would ultimately be proposed upstream. You'll find shortcuts, rough edges, and code that's optimized for rapid iteration rather than long-term maintainability.

The implementation proposed for upstream integration will be developed to the standards expected of production software, with the appropriate architecture, testing, review, and documentation. 

## Vulnerability Reporting

In the future, we would like to receive thorough reviews by bug-hunting teams, but currently we would kindly ask that you direct your vulnerability discovery efforts to: https://github.com/zcashfoundation/zebra

# crosslink_monolith

A subtree'd monorepo for all crosslink code/dependencies

## Send wallet funds to a transparent address

In the Crosslink Visualizer, open **Your Wallet → Send**, paste or enter your
Crosslink testnet transparent address, enter an amount in cTAZ (up to eight decimal
places), and select **Send Payment**. The recipient receives that amount; the
network fee is paid in addition, and change returns to your shielded wallet.
For example, with 5 cTAZ available, sending 4.99 cTAZ leaves room for the fee.
Only spendable funds are used; shielded receipts need three confirmations.
The dialog shows submission errors or the submitted transaction ID.

The same feature is available through `wallet_basic_send`, with the amount in
zatoshis (100,000,000 zatoshis = 1 cTAZ):

```sh
curl -sS http://127.0.0.1:8232 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"wallet_basic_send","params":[499000000,"YOUR_CROSSLINK_TESTNET_TRANSPARENT_ADDRESS"]}'
```

Transparent payments publish the recipient address and amount. Both P2PKH and
P2SH transparent addresses are supported. Unified addresses retain their shielded
receiver preference. Mainnet addresses, Sapling addresses, and TEX addresses are
not accepted by this testnet wallet. cTAZ remains on the Crosslink network; sending
to a transparent address does not convert it into mainnet ZEC.

## Run the Crosslink Zebra RPC node with Docker

From the repository root, build and start the single Zebra node container:

```sh
docker compose up --build -d
docker compose logs -f zebra
```

The development endpoints are bound to localhost only:

- JSON-RPC: `http://127.0.0.1:8232`
- indexer gRPC: `http://127.0.0.1:8230`

Check JSON-RPC after the node starts:

```sh
curl -sS http://127.0.0.1:8232 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getblockchaininfo","params":[]}'
```

Stop the node without deleting its chain state:

```sh
docker compose down
```

Delete the Docker chain state as well:

```sh
docker compose down -v
```
