# Rust project checks

set positional-arguments
set shell := ["bash", "-euo", "pipefail", "-c"]

# List available commands
default:
    @just --list

# Run format, clippy-fix, and build in parallel
[parallel]
check: format clippy-fix build

# Run check and fail if there are uncommitted changes (for CI)
check-ci: check
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "Error: check caused uncommitted changes"
        echo "Run 'just check' locally and commit the results"
        git diff --stat
        exit 1
    fi

# Format Rust files
format:
    @cargo fmt --all

# Run clippy with all warnings
clippy:
    @cargo clippy --quiet -- -W clippy::all 2>&1 | { grep -v "^0 errors" || true; }

# Auto-fix clippy warnings
clippy-fix:
    @cargo clippy --fix --allow-dirty --quiet -- -W clippy::all 2>&1 | { grep -v "^0 errors" || true; }

# Build the project
build:
    cargo build --all

# Install release binary globally
install:
    cargo install --offline --path . --locked

# Install debug binary globally via symlink
install-dev:
    cargo build && ln -sf $(pwd)/target/debug/claude-history ~/.cargo/bin/claude-history

# Run the application
run *ARGS:
    cargo run -- "$@"

# Release a new patch version
release:
    @just _release patch

# Internal release helper
_release bump:
    @cargo-release {{bump}}
