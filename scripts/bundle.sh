#!/usr/bin/env bash
# Bundle into target/bundle. bundle-cef-app needs CEF_PATH/TMPDIR in env
# (it does not read .cargo/config.toml; /tmp is often too small for CEF).
set -euo pipefail

package="${1:?usage: scripts/bundle.sh <package>}"
shift

export CEF_PATH="${CEF_PATH:-$HOME/.local/share/cef}"
export TMPDIR="${TMPDIR:-$HOME/Work/.cargo-tmp}"
mkdir -p "$TMPDIR"
export PATH="$HOME/.local/share/cargo-cef-tools/bin:$PATH"

cd "$(dirname "$0")/.."
bundle-cef-app "$package" -o target/bundle --release "$@"
# EasyList / EasyPrivacy for the content-filter engine (Settings → Blocking).
if [[ -d data/filters ]]; then
  mkdir -p target/bundle/filters
  cp -a data/filters/. target/bundle/filters/
fi
echo "Run it: target/bundle/$package"
