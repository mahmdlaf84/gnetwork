#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
TESTNET_DIR="${PROJECT_ROOT}/.gpang-testnet"
LEDGER_FILE="${TESTNET_DIR}/ledger.json"

mkdir -p "${TESTNET_DIR}"

echo "Starting GPANG testnet with ledger at ${LEDGER_FILE}"
cd "${PROJECT_ROOT}"
RUST_LOG="${RUST_LOG:-info}" cargo run -p rpc -- --testnet --ledger "${LEDGER_FILE}"
