#!/usr/bin/env bash
# Install an extracted release tarball: ./install-release.sh [tarball-dir]
# Copies to ~/.local/share/spatial-browser (unless run from there), adds a
# ~/.local/bin/spatial-browser launcher, hicolor icon and .desktop entry.
set -euo pipefail

src="$(cd "${1:-$(dirname "$0")}" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
dest="$data/spatial-browser"
bin_dir="$HOME/.local/bin"

[ -x "$src/compositor" ] || { echo "error: $src/compositor not found" >&2; exit 1; }
[ -f "$src/icon.svg" ] || { echo "error: $src/icon.svg not found" >&2; exit 1; }

if [ "$src" != "$dest" ]; then
    mkdir -p "$dest"
    cp -a "$src/." "$dest/"
fi

mkdir -p "$bin_dir"
cat >"$bin_dir/spatial-browser" <<LAUNCH
#!/usr/bin/env bash
exec "$dest/compositor" "\$@"
LAUNCH
chmod +x "$bin_dir/spatial-browser"

# desktop-common.sh is shipped in the tarball by bundle.sh.
. "$dest/desktop-common.sh"
install_desktop "$dest/icon.svg" "$bin_dir/spatial-browser"
