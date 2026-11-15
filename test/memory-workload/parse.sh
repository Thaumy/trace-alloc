#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname -- "${BASH_SOURCE[0]}")"

DUMP="${1:-ta.dump}"
OUTPUT="${2:-ta.dump.parsed}"

cargo run --release --manifest-path ../../Cargo.toml --bin dp -- "$DUMP" > "$OUTPUT"
