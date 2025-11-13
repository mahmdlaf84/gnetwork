# GPANG Network — Decentralized GPU & AI Agent Blockchain

GPANG Network is a Rust-based prototype for a decentralized GPU marketplace with an economic feedback loop. It combines a JSON-persisted ledger, a task-proof consensus loop capable of aggregating tens of millions of tokens per second, a command-line interface, and a lightweight explorer dashboard.

## Workspace Layout

- `ledger/` — Core state machine with token accounting, smart contract and token registries, provider catalogues, task tracking, and JSON persistence helpers.
- `rpc/` — Axum-based REST API and task-proof consensus loop producing blocks every 3 seconds.
- `gpang-cli/` — Command line tool for managing providers, nodes, accounts, contracts, custom tokens, treasury flows, and submitting tasks.
- `explorer/` — Static HTML/Tailwind dashboard that consumes the REST API for network visibility.
- `scripts/` — Convenience scripts, including `start-node.sh` for launching the RPC node.
- `ledger_v35.json` — On-disk ledger snapshot used by the node and CLI.

## Prerequisites

- Rust 1.74+ with the `cargo` build tool
- Node.js is **not** required (the explorer is static HTML)

## Building

```bash
cargo build --release
```

The command builds all workspace members in release mode. The ledger crate is pure library code, while the `rpc` and `gpang-cli` crates compile into executables that the helper scripts launch directly.

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
./target/release/rpc --testnet --ledger .gpang-testnet/ledger.json --listen 127.0.0.1:8080
```

Use the standard CLI commands to inspect balances, register additional nodes, or submit tasks against the seeded infrastructure.

## Node Roles and Stake Requirements

GPANG distinguishes four decentralized node roles that collaborate to keep the network scalable and tamper-resistant:

- **Validator nodes** cast HotStuff-style votes on each block. They must maintain at least `100,000,000` AIA staked before registration is accepted, guaranteeing economic skin-in-the-game.
- **Compute nodes** execute user workloads, generate task proofs, and are the only nodes counted toward task slot fairness metrics.
- **Scheduler nodes** receive plans from assignment nodes and fan tasks out to the highest-scoring compute providers while tracking how many jobs they coordinate.
- **Assignment nodes** analyze global load, craft decentralized scheduling plans, and forward them to schedulers for execution.

Every node shares a unified registration flow: select the appropriate role during `gpang node register` and the CLI will auto-discover hardware metrics, GPU vendor details (NVIDIA, AMD, or Apple), and stream them to the ledger. Explore role-specific counts, validator stake thresholds, and coordination statistics from the web dashboard summary cards.

## CLI Usage

```bash
./scripts/gpang --help
```

The helper script compiles the `gpang-cli` binary in release mode on first use and reuses the artifact for subsequent commands, so you never need to invoke `cargo run` manually.

### Examples

Register a GPU provider and set model profiles:

```bash
./scripts/gpang provider upsert-gpu \
  --id prov-fast \
  --endpoint http://127.0.0.1:8080 \
  --owner alice \
  --region AP-SEA \
  --gpu-model "RTX 4090" \
  --vram-gb 24

./scripts/gpang provider set-models \
  --id prov-fast \
  --profiles '[{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}]'
```

Register a node and submit a task:

```bash
./scripts/gpang node register \
  --owner alice \
  --region AP-SEA \
  --role compute \
  --llm-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}'

./scripts/gpang task submit \
  --owner alice \
  --content-hash Qm123... \
  --total-tokens 300000 \
  --target-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}' \
  --providers '["prov-fast","prov-us","prov-eu"]' \
  --preferred-region AP-SEA

# Submit a chat-oriented task that aggregates answers from multiple nodes
./scripts/gpang task submit \
  --owner alice \
  --content-hash chat-001 \
  --total-tokens 120000 \
  --target-profile '{"model_id":"qwen2.5-7b","quant":"int4","max_ctx":8192,"vram_req_gb":12,"throughput_tok_s":220000}' \
  --providers '["prov-fast","prov-us","prov-eu"]' \
  --mode chat \
  --chat-prompt "Summarize the latest GPU architecture breakthroughs across vendors"

# Inspect node inventories and earnings without leaving the CLI
./scripts/gpang node list
./scripts/gpang node list --owner alice
./scripts/gpang node show --node-id node-1
./scripts/gpang node earnings --node-id node-1

# Submit a task-proof commitment to advance consensus
./scripts/gpang task proof \
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
./scripts/gpang account mint --to alice --token AIA --amount 100000
./scripts/gpang account stake --owner alice --amount 50000
```

To promote a validator, stake the required AIA first and then register with `--role validator`. Registrations fail fast if the owner lacks the minimum bonded balance, ensuring validators always secure the chain with collateral.

Deploy and exercise a smart contract:

```bash
# Deploy inline code (or use --code-path to read from disk)
./scripts/gpang contract deploy \
  --owner foundation \
  --name welcome-airdrop \
  --code "fn distribute() { /* demo */ }" \
  --metadata '{"description":"genesis faucet"}'

# Deploy a Solana BPF program (bytecode is base64-encoded automatically)
./scripts/gpang contract deploy \
  --owner foundation \
  --name solana-router \
  --runtime solana \
  --program-id 3nmyD5Zb9WJ6A3W5v1VXnMqJ8C8Yg4k5tA8W9rUu1xYg \
  --bytecode-path ./artifacts/solana_router.so \
  --metadata '{"description":"bridges Solana-native flows"}'

# Invoke the contract with an immutable payload
./scripts/gpang contract execute \
  --contract-id contract-1 \
  --caller alice \
  --method distribute \
  --payload '{"target":"new-node"}'
```

Create a platform-wide token, mint supply, and route balances through the treasury:

```bash
./scripts/gpang token create \
  --symbol GPANGX \
  --name "GPANG Expansion" \
  --owner foundation \
  --initial-supply 1000000000 \
  --platform true

./scripts/gpang token mint \
  --symbol BLDR \
  --to builder \
  --amount 5000000 \
  --authority builder

./scripts/gpang treasury deposit --from builder --symbol BLDR --amount 100000
./scripts/gpang treasury withdraw --to alice --symbol AIA --amount 25000 --authority alice
```

## Gas Accounting & Zero-Proof Validation

- **Universal gas metering** — Every transaction now consumes intrinsic gas units that reflect its execution complexity (e.g. transfers cost 25,000 units, task-proof commits cost 55,000 units). The RPC layer estimates the limit automatically, but advanced operators can override `gas_price`, `gas_limit`, or `gas_payer` when calling the JSON API. Ensure the payer holds at least `gas_price * intrinsic_gas` AIA before submitting very small transactions (minting credits the balance first, then deducts gas).
- **Treasury funding** — Gas fees are denominated in AIA, deducted from the payer after the state transition is applied, and routed to the treasury. The explorer surfaces the running `gas_collected` counter alongside the base AIA treasury balance so network operators can audit utilization.
- **Deterministic zero proofs** — Each transaction carries a `zero_proof` object that hashes the payload, gas metadata, and payer into a deterministic digest. The ledger re-computes and verifies this digest before accepting a transaction, providing lightweight tamper detection without external cryptography libraries.
- **Automatic payer inference** — By default the RPC service infers a payer (e.g. `from` on transfers, provider owners on segment receipts) so CLI users are not forced to pass extra flags, while power users can still provide explicit overrides for multi-account workflows.

## Smart Contracts, Custom Tokens & Treasury Controls

- **Immutable contract registry** — Users can deploy arbitrary smart contract source via `contract deploy`. The ledger persists the code, metadata, and hash alongside a deterministic identifier and exposes `/contracts` plus `/contracts/events` so explorers can audit execution history. Every invocation emits an immutable event with the payload digest, preserving tamper evidence for task-proof consensus.
- **Token factory with platform support** — `token create` registers fungible assets with configurable decimals, optional platform status, and optional linkage to a contract. Platform tokens automatically fund the treasury, while community tokens credit the issuer. Minting requires the recorded owner as authority so supplies remain provably controlled.
- **Treasury deposits and withdrawals** — The treasury tracks balances for built-in and custom tokens. Operators can push rewards into reserves (`treasury deposit`) and withdraw under stake- or owner-based authorization (`treasury withdraw`). The explorer exposes aggregate balances, while the ledger enforces stake-backed approvals for native assets and owner checks for custom ones.

## Explorer

Open `explorer/index.html` in a browser. The dashboard polls the REST API every 5 seconds to visualize blocks, providers, nodes, tasks, smart contracts, token registries, and treasury balances.

## REST API Overview

- `GET /` — Network summary
- `GET /blocks` — Block history
- `GET /accounts` — Accounts and balances
- `GET /providers` — Provider registry
- `GET /nodes` — Registered nodes
- `GET /node/:node_id` — Detailed node record with hardware, metrics, and role data
- `GET /node/:node_id/earnings` — Summary of cumulative node rewards and segment history
- `GET /accounts/:account_id/nodes` — Nodes registered by a specific account plus aggregated earnings
- `GET /tasks` — Task ledger
- `GET /contracts` — Smart contract registry and metadata
- `GET /contracts/events` — Immutable execution log entries
- `GET /tokens` — Custom and platform token definitions
- `GET /treasury` — Treasury balances
- `POST /tx` — Submit any token transaction (`TransactionKind` payload plus optional `gas_price`, `gas_limit`, `gas_payer` overrides)
- `POST /provider/upsert` — Register or update provider metadata
- `POST /provider/models` — Configure provider model profiles
- `POST /provider/heartbeat` — Refresh provider heartbeat
- `POST /node/register` — Register node metadata
- `POST /node/status` — Update node online state
- `POST /task/submit` — Submit a new inference task (supports `mode` = `batch` or `chat` plus optional `chat_prompt`, returns scheduler hints)
- `POST /consensus/task-proof` — Publish a task-proof commitment used for block production (accepts optional `node_id`, `output_digest`, `chat_response`, and gas override fields)

## Consensus Loop

A background task ticks every 3 seconds:

1. Drains mempool transactions accepted via `/tx` and `/consensus/task-proof`.
2. Elects a high-stake, high-throughput node as task-proof leader using task share aware weighting so under-served nodes gain additional chances to produce blocks.
3. Converts validated proofs into `segment_receipt` + `payout` transactions, then appends staking reward payouts computed from `reward_bps`.
4. Commits a new block with the collected transactions.
5. Persists the ledger snapshot to `ledger_v35.json`.

This HotStuff-Pro inspired loop runs as a single validator today, but the task-proof path mirrors a decentralized HotStuff-Pro deployment. Consensus rounds track cumulative throughput (`target_tokens_per_sec` defaults to 10,000,000) and can scale to 100,000,000 parallel nodes submitting proofs. The ledger records how many task slots each node captures, keeps an average tasks-per-node benchmark, and penalizes leaders that have already claimed disproportionate work so scheduling and consensus remain decentralized as throughput increases.

## Persistence

All state is kept in `ledger_v35.json`. The file grows as blocks, providers, tasks, and accounts evolve. Backups can be performed by copying the file; new nodes bootstrap by reading the JSON snapshot.

## License

MIT
