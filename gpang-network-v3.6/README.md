# GPANG Network — Decentralized GPU & AI Agent Blockchain

GPANG Network is a Rust-based prototype for a decentralized GPU marketplace with an economic feedback loop. It combines a JSON-persisted ledger, a task-proof consensus loop capable of aggregating tens of millions of tokens per second, a command-line interface, and a lightweight explorer dashboard.

## Workspace Layout

- `ledger/` — Core state machine with token accounting, provider registry, task tracking, and JSON persistence helpers.
- `rpc/` — Axum-based REST API and task-proof consensus loop producing blocks every 3 seconds.
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

## Starting a Local Testnet

For an isolated playground with deterministic genesis data, launch the RPC service in **testnet** mode. This creates a `.gpang-testnet/ledger.json` snapshot seeded with funded accounts, multi-region providers, and validator nodes so you can interact with the network immediately.

```bash
./scripts/start-testnet.sh
```

Alternatively run the RPC binary directly:

```bash
cargo run -p rpc -- --testnet --ledger .gpang-testnet/ledger.json --listen 127.0.0.1:8080
```

Use the standard CLI commands to inspect balances, register additional nodes, or submit tasks against the seeded infrastructure.

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
  --owner alice \
  --region AP-SEA \
  --llm-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}'

cargo run -p gpang-cli -- task submit \
  --owner alice \
  --content-hash Qm123... \
  --total-tokens 300000 \
  --target-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}' \
  --providers '["prov-fast","prov-us","prov-eu"]' \
  --preferred-region AP-SEA

# Submit a chat-oriented task that aggregates answers from multiple nodes
cargo run -p gpang-cli -- task submit \
  --owner alice \
  --content-hash chat-001 \
  --total-tokens 120000 \
  --target-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}' \
  --providers '["prov-fast","prov-us","prov-eu"]' \
  --mode chat \
  --chat-prompt "Summarize the latest GPU architecture breakthroughs across vendors"

# Submit a task-proof commitment to advance consensus
cargo run -p gpang-cli -- task proof \
  --round 1 \
  --task-id task-1 \
  --segment-id seg-1 \
  --provider-id prov-fast \
  --latency-ms 420 \
  --throughput-tok-s 250000 \
  --tokens-processed 100000 \
  --region AP-SEA \
  --signature demo-proof \
  --node-id node-1 \
  --chat-response "NVIDIA and AMD both expanded HBM3e roadmaps; Apple doubled unified memory bandwidth"
```

When registering or updating a node, the CLI auto-discovers CPU, GPU (covering NVIDIA, AMD, and Apple silicon), memory, disk, and network telemetry from the host. A deterministic fingerprint is derived from this hardware snapshot so the ledger can mint a unique on-chain identity per machine while streaming utilization metrics to the explorer dashboard in real time.

Chat-mode tasks fan out to the top scheduled providers, capture their conversational responses, and persist a consolidated summary on-chain. The explorer highlights the aggregate along with contributor counts so operators can audit how the merged answer was produced.

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
- `POST /task/submit` — Submit a new inference task (supports `mode` = `batch` or `chat` plus optional `chat_prompt`, returns scheduler hints)
- `POST /consensus/task-proof` — Publish a task-proof commitment used for block production (accepts optional `node_id`, `output_digest`, and `chat_response` fields)

## Consensus Loop

A background task ticks every 3 seconds:

1. Drains mempool transactions accepted via `/tx` and `/consensus/task-proof`.
2. Elects a high-stake, high-throughput node as task-proof leader and aggregates submitted proofs.
3. Converts validated proofs into `segment_receipt` + `payout` transactions, then appends staking reward payouts computed from `reward_bps`.
4. Commits a new block with the collected transactions.
5. Persists the ledger snapshot to `ledger_v35.json`.

This HotStuff-Pro inspired loop runs as a single validator today, but the task-proof path mirrors a decentralized HotStuff-Pro deployment. Consensus rounds track cumulative throughput (`target_tokens_per_sec` defaults to 10,000,000) and can scale to 100,000,000 parallel nodes submitting proofs.

## Persistence

All state is kept in `ledger_v35.json`. The file grows as blocks, providers, tasks, and accounts evolve. Backups can be performed by copying the file; new nodes bootstrap by reading the JSON snapshot.

## License

MIT
