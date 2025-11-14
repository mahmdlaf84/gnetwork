#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
LEDGER_FILE="${PROJECT_ROOT}/ledger_v35.json"
RPC_BINARY="${PROJECT_ROOT}/target/release/rpc"

if [ ! -f "$LEDGER_FILE" ]; then
  echo "Initializing ledger at $LEDGER_FILE"
  echo '{}' > "$LEDGER_FILE"
fi

cd "$PROJECT_ROOT"
if [ ! -x "$RPC_BINARY" ]; then
  cargo build -p rpc --release
fi

export RUST_LOG="${RUST_LOG:-info}"
exec "$RPC_BINARY"
