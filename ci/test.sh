#!/bin/sh

main() {
    set -ex
    workdir="$(mktemp -d)"
    trap cleanup EXIT
    cargo_clippy
    cargo_test
    cargo_build
}

cargo_clippy() {
    cargo clippy --workspace --quiet --all-features --all-targets -- --deny warnings
}

cargo_test() {
    cargo test --workspace --quiet --no-fail-fast --all-features
}

cargo_build() {
    cargo build --no-default-features
    cargo build --no-default-features --features std
}

cleanup() {
    rm -rf "$workdir"
}

main "$@"
