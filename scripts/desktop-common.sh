# Shared by install.sh / install-release.sh. Usage: install_desktop <icon.svg> <exec>
install_desktop() {
    local icon_src="$1" exec_cmd="$2"
    local data="${XDG_DATA_HOME:-$HOME/.local/share}"
    local apps_dir="$data/applications" hicolor="$data/icons/hicolor"
    local icon_dir="$hicolor/scalable/apps"
    mkdir -p "$apps_dir" "$icon_dir"
    # Hicolor copy is a convenience only; Icon= uses the absolute path
    # (a partial icon-theme.cache without index.theme hides the icon).
    install -m 0644 "$icon_src" "$icon_dir/spatial-browser.svg"
    cat >"$apps_dir/spatial-browser.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Spatial Browser
Comment=Multi-page spatial browser (pannable/zoomable canvas of pages)
Exec=$exec_cmd
Icon=$icon_src
Terminal=false
Categories=Network;WebBrowser;
StartupWMClass=spatial-browser
StartupNotify=true
DESKTOP
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database "$apps_dir" || true
    fi
    echo "installed: $apps_dir/spatial-browser.desktop (icon: $icon_dir/spatial-browser.svg)"
}
