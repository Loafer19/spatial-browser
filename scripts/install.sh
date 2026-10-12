#!/usr/bin/env bash
# Desktop entry + hicolor icon for this source checkout (Exec needs an absolute path).
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
. "$repo_root/scripts/desktop-common.sh"

install_desktop "$repo_root/data/icon.svg" "$repo_root/scripts/run.sh compositor"

if [ ! -x "$repo_root/target/bundle/compositor" ]; then
    echo "note: no bundled binary yet — run ./scripts/bundle.sh compositor first" >&2
fi
