#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TESTNET_DIR="${PROJECT_ROOT}/.gpang-testnet"
LEDGER_FILE="${TESTNET_DIR}/ledger.json"
RPC_BINARY="${PROJECT_ROOT}/target/release/rpc"

mkdir -p "${TESTNET_DIR}"

cd "${PROJECT_ROOT}"
if [ ! -x "$RPC_BINARY" ]; then
  cargo build -p rpc --release
fi

echo "Starting GPANG testnet with ledger at ${LEDGER_FILE}"
export RUST_LOG="${RUST_LOG:-info}"
exec "$RPC_BINARY" --testnet --ledger "${LEDGER_FILE}" "$@"
