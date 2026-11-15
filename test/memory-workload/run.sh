#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname -- "${BASH_SOURCE[0]}")"

export TRACE_ALLOC_DUMP_PATH="${TRACE_ALLOC_DUMP_PATH:-ta.dump}"
export TRACE_ALLOC_DUMP_BUF_SIZE_BYTES="${TRACE_ALLOC_DUMP_BUF_SIZE_BYTES:-0}"
export TRACE_ALLOC_SAMPLE_PERIOD_BYTES="${TRACE_ALLOC_SAMPLE_PERIOD_BYTES:-1048576}"
export TRACE_ALLOC_TEST_MIB="${TRACE_ALLOC_TEST_MIB:-128}"

exec cargo run --release -- "$@"
