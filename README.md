# Important Disclaimer About This Repository

This repository exists to prove ideas, not to demonstrate production engineering. It is a prototype designed to let us move quickly and validate the Crosslink design.

It is **not** production-ready code, and it is **not** what would ultimately be proposed upstream. You'll find shortcuts, rough edges, and code that's optimized for rapid iteration rather than long-term maintainability.

The implementation proposed for upstream integration will be developed to the standards expected of production software, with the appropriate architecture, testing, review, and documentation. 

## Vulnerability Reporting

In the future, we would like to receive thorough reviews by bug-hunting teams, but currently we would kindly ask that you direct your vulnerability discovery efforts to: https://github.com/zcashfoundation/zebra

# crosslink_monolith

A subtree'd monorepo for all crosslink code/dependencies

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
