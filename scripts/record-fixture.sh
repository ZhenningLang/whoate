#!/usr/bin/env bash
set -euo pipefail
exec </dev/null
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel)}"
cd "$REPO_ROOT"
exec cargo run --quiet -p whoate-core --release --locked --example record_fixture -- "$@"
