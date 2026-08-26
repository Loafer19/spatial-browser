# spatial-browser

Personal browser shell: pages as pannable/zoomable tiles on a canvas
instead of flat tabs.

![Canvas with a focused page, a background page, and the history/bookmarks/downloads/shortcuts lists open](.github/screenshots/canvas.png)

**CEF** off-screen rendering · **wgpu** compositor · **Linux x86_64**

## Docs

→ **[Wiki](https://github.com/Loafer19/spatial-browser/wiki)**

| | |
|---|---|
| [Getting started](https://github.com/Loafer19/spatial-browser/wiki/Getting-Started) | Install, run, Chrome bookmark import |
| [Architecture](https://github.com/Loafer19/spatial-browser/wiki/Architecture) | Crates, CEF OSR, canvas UI model |
| [Canvas & shortcuts](https://github.com/Loafer19/spatial-browser/wiki/Canvas-and-Shortcuts) | Pan/zoom/touch + hotkeys (`F1` in-app) |
| [Scripts & styles](https://github.com/Loafer19/spatial-browser/wiki/Userscripts) | Userscripts + CSS (`spatial-ui` for built-in pages) |
| [Features](https://github.com/Loafer19/spatial-browser/wiki/Features) | Bookmarks, workspaces, reader, adblock, … |
| [Configuration](https://github.com/Loafer19/spatial-browser/wiki/Configuration) | Config paths, settings, env overrides |
| [Limitations](https://github.com/Loafer19/spatial-browser/wiki/Limitations) | No Chrome extensions, known CEF quirks |
| [Building & releasing](https://github.com/Loafer19/spatial-browser/wiki/Building-and-Releasing) | CEF pin, bundle, version tags |

## Quick start

```sh
# One-time CEF tools (version must match crates/*/Cargo.toml)
cargo install cef --version 151.8.0+151.3.24 --locked \
  --root ~/.local/share/cargo-cef-tools
cargo install export-cef-dir --version 151.8.0+151.3.24 --locked \
  --root ~/.local/share/cargo-cef-tools
~/.local/share/cargo-cef-tools/bin/export-cef-dir --force ~/.local/share/cef

cargo build
./scripts/bundle.sh compositor
./scripts/run.sh compositor      # auto-relaunches on the known CEF crash
./scripts/install.sh             # optional desktop launcher
```

Or grab a tarball from [Releases](https://github.com/Loafer19/spatial-browser/releases).

## License

MIT — see [LICENSE](LICENSE).
