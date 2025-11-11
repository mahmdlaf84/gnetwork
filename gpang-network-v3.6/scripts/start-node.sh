#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
LEDGER_FILE="${PROJECT_ROOT}/ledger_v35.json"

if [ ! -f "$LEDGER_FILE" ]; then
  echo "Initializing ledger at $LEDGER_FILE"
  echo '{}' > "$LEDGER_FILE"
fi

cd "$PROJECT_ROOT"
RUST_LOG=info cargo run -p rpc --release
