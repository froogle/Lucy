#!/usr/bin/env bash
# Every gate a change must pass. CI and the pre-commit hook both run this.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
# Includes the JSON Schema drift check (crates/lucy-ipc/tests/schema.rs).
cargo test --locked --workspace
scripts/check-client-deps.sh
scripts/check-glossary-terms.sh
scripts/check-lint-allows.sh
scripts/check-test-sleeps.sh
cargo deny check licenses
