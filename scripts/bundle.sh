#!/usr/bin/env bash
# Bundle a package into target/bundle, including the CEF subprocess
# helper. `cargo build`/`cargo run` pick up CEF_PATH from
# .cargo/config.toml automatically, but `bundle-cef-app` is a separately
# installed tool (cargo install cef --version 151.8.0+151.3.24 --locked
# --root ~/.local/share/cargo-cef-tools) invoked directly as a shell
# command, so it doesn't see that file — it needs CEF_PATH in its own
# process environment. TMPDIR is also needed: cargo install's temp build
# dir defaults to /tmp, which is a small tmpfs here and overflows on
# CEF's dependency tree.
set -euo pipefail

package="${1:?usage: scripts/bundle.sh <package>}"
shift

export CEF_PATH="${CEF_PATH:-$HOME/.local/share/cef}"
export TMPDIR="${TMPDIR:-$HOME/Work/.cargo-tmp}"
mkdir -p "$TMPDIR"
export PATH="$HOME/.local/share/cargo-cef-tools/bin:$PATH"

cd "$(dirname "$0")/.."
bundle-cef-app "$package" -o target/bundle --release "$@"
echo "Run it: target/bundle/$package"
