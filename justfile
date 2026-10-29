# Recipes for rev. Run `just` to list them.

set positional-arguments

cache := join(cache_directory(), "rev")

alias b := build
alias r := run

_default:
    @just --list --unsorted

# Run the TUI, e.g. `just run CSCE 221 --term "fall 2026"`
run *args:
    @cargo run --release --quiet -- "$@"

# Build the optimized binary into target/release/
build:
    cargo build --release

# Offline unit tests; extra args go to `cargo test` (e.g. a test name filter)
test *args:
    cargo test "$@"

# Tests that hit AggieSB+, anex, and Rate My Professors for real
live *args:
    cargo test "$@" -- --ignored --nocapture --test-threads=1

# Clippy, with warnings treated as errors
lint:
    cargo clippy --all-targets -- -D warnings

# Format the code
fmt:
    cargo fmt

# Formatting, lints, and unit tests: run before committing
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Delete cached AggieSB+, anex, and RMP responses
clear-cache:
    rm -rf '{{cache}}'

# Print the path to the `rev` cache
show-cache:
    @echo '{{cache}}'
