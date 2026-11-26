#!/usr/bin/env bash
# Runs every quality gate, in order. Stops at the first failure.
#
#   scripts/check.sh
#
# Coverage and complexity need two extra tools (see README, "Quality checks").
# If they aren't installed, those steps are skipped with a note.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

# Locally a missing optional tool is skipped with a hint. In CI (where the
# CI environment variable is set) every tool must be present.
missing() {
    echo "skipped: $1"
    if [ -n "${CI:-}" ]; then
        echo "error: CI must run every check" >&2
        exit 1
    fi
}

step "Format (rustfmt)"
cargo fmt --all -- --check

step "Lint (clippy; warnings are errors)"
cargo clippy --workspace --all-targets -- -D warnings

step "Tests"
cargo test --workspace --quiet

step "Docs (rustdoc; warnings are errors)"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet

step "Coverage (cargo-llvm-cov; at least 90% of lines)"
if command -v cargo-llvm-cov >/dev/null; then
    # Clear old coverage data first, so results never mix with earlier builds.
    cargo llvm-cov clean --workspace
    cargo llvm-cov --workspace --summary-only --fail-under-lines 90
else
    missing "install with 'cargo install cargo-llvm-cov' and 'rustup component add llvm-tools-preview'"
fi

step "Cyclomatic complexity (lizard; no function over 10)"
if command -v lizard >/dev/null; then
    # Two separate lines, not `lizard ... && echo ok`: with `set -e`, bash
    # ignores a failure on the left of `&&`, which would let violations pass.
    lizard --languages rust --CCN 10 --warnings_only crates/
    echo "ok: no function over CCN 10"
else
    missing "install with 'pipx install lizard' (or 'pip install lizard')"
fi

step "Agent tools (their own tests)"
if command -v python3 >/dev/null; then
    python3 -m unittest discover -s agent-tools -p 'test_*.py' --quiet
else
    missing "install python3 to test the agent tools in agent-tools/"
fi

printf '\nAll checks passed.\n'
