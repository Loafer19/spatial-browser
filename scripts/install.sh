#!/usr/bin/env bash
# Installs (or updates) a desktop-launcher entry for the app-grid/launcher
# (Super -> Apps, wofi, rofi, ...) pointing at *this* checkout — regenerated
# every run so moving the repo and re-running this script fixes the path.
# Desktop Entry Exec/Icon keys don't expand `~` or env vars, so both need
# to be absolute paths baked in at install time; that's why this is a
# script and not a static .desktop file shipped in the repo.
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
