#!/usr/bin/env bash
# Write a .desktop entry for this checkout (Exec/Icon need absolute paths).
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
apps_dir="$HOME/.local/share/applications"
desktop_file="$apps_dir/spatial-browser.desktop"

mkdir -p "$apps_dir"
cat >"$desktop_file" <<EOF
[Desktop Entry]
Type=Application
Name=Spatial Browser
Comment=Multi-page spatial browser (pannable/zoomable canvas of pages)
Exec=$repo_root/scripts/run.sh compositor
Icon=$repo_root/data/icon.svg
Terminal=false
Categories=Network;WebBrowser;
StartupWMClass=spatial-browser
StartupNotify=true
EOF

if [ ! -x "$repo_root/target/bundle/compositor" ]; then
    echo "note: no bundled binary yet — run ./scripts/bundle.sh compositor first" >&2
fi

if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$apps_dir"
fi

echo "installed: $desktop_file"
