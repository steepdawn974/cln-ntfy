#!/bin/bash
set -x
script_dir=$(dirname -- "$(readlink -f -- "$0")")
cargo_toml_path="$script_dir/../Cargo.toml"

# Install a recent Rust toolchain if rustc is missing, then build the plugin.
if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi
cargo build --manifest-path="$cargo_toml_path"
