# GPANG Network — Decentralized GPU & AI Agent Blockchain

GPANG Network is a Rust-based prototype for a decentralized GPU marketplace with an economic feedback loop. It combines a JSON-persisted ledger, a HotStuff-inspired single-node block producer, a command-line interface, and a lightweight explorer dashboard.

## Workspace Layout

- `ledger/` — Core state machine with token accounting, provider registry, task tracking, and JSON persistence helpers.
- `rpc/` — Axum-based REST API and consensus loop producing blocks every 3 seconds.
- `gpang-cli/` — Command line tool for managing providers, nodes, accounts, and submitting tasks.
- `explorer/` — Static HTML/Tailwind dashboard that consumes the REST API for network visibility.
- `scripts/` — Convenience scripts, including `start-node.sh` for launching the RPC node.
- `ledger_v35.json` — On-disk ledger snapshot used by the node and CLI.

## Prerequisites

- Rust 1.74+ with the `cargo` build tool
- Node.js is **not** required (the explorer is static HTML)

## Building

```bash
cargo build
```

The command builds all workspace members. The ledger crate is pure library code, while the `rpc` and `gpang-cli` crates compile into executables.

## Running the Node

```bash
./scripts/start-node.sh
```

The script initializes `ledger_v35.json` if missing and starts the RPC service on `0.0.0.0:8080`. Blocks are produced every 3 seconds, even when the mempool is empty. Transactions submitted via the CLI or HTTP API are batched into the next block and persisted to disk.

### Environment variables

- `LEDGER_FILE` — Override the ledger path (defaults to `ledger_v35.json`).
- `RUST_LOG` — Standard `tracing` filter (defaults to `info` in the script).

## CLI Usage

```bash
cargo run -p gpang-cli -- --help
```

### Examples

Register a GPU provider and set model profiles:

```bash
cargo run -p gpang-cli -- provider upsert-gpu \
  --id prov-fast \
  --endpoint http://127.0.0.1:8080 \
  --owner alice \
  --region AP-SEA \
  --gpu-model "RTX 4090" \
  --vram-gb 24

cargo run -p gpang-cli -- provider set-models \
  --id prov-fast \
  --profiles '[{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}]'
```

Register a node and submit a task:

```bash
cargo run -p gpang-cli -- node register \
  --id node-1 \
  --owner alice \
  --gpu-model "RTX 4090" \
  --region AP-SEA \
  --llm-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}'

cargo run -p gpang-cli -- task submit \
  --owner alice \
  --content-hash Qm123... \
  --total-tokens 300000 \
  --target-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}' \
  --providers '["prov-fast","prov-us","prov-eu"]'
```

Mint tokens and stake:

```bash
cargo run -p gpang-cli -- account mint --to alice --token AIA --amount 100000
cargo run -p gpang-cli -- account stake --owner alice --amount 50000
```

## Explorer

Open `explorer/index.html` in a browser. The dashboard polls the REST API every 5 seconds to visualize blocks, providers, nodes, tasks, and treasury balances.

## REST API Overview

- `GET /` — Network summary
- `GET /blocks` — Block history
- `GET /accounts` — Accounts and balances
- `GET /providers` — Provider registry
- `GET /nodes` — Registered nodes
- `GET /tasks` — Task ledger
- `GET /treasury` — Treasury balances
- `POST /tx` — Submit any token transaction (`TransactionKind` payload)
- `POST /provider/upsert` — Register or update provider metadata
- `POST /provider/models` — Configure provider model profiles
- `POST /provider/heartbeat` — Refresh provider heartbeat
- `POST /node/register` — Register node metadata
- `POST /node/status` — Update node online state
- `POST /task/submit` — Submit a new inference task

## Consensus Loop

A background task ticks every 3 seconds:

1. Drains mempool transactions accepted via `/tx`.
2. Appends staking reward payouts computed from `reward_bps`.
3. Commits a new block with the collected transactions.
4. Persists the ledger snapshot to `ledger_v35.json`.

This HotStuff-Pro inspired loop runs as a single validator but exposes hooks to plug in external QC logic by replacing `ledger.commit_block` with networked coordination.

## Persistence

All state is kept in `ledger_v35.json`. The file grows as blocks, providers, tasks, and accounts evolve. Backups can be performed by copying the file; new nodes bootstrap by reading the JSON snapshot.

## License

MIT
