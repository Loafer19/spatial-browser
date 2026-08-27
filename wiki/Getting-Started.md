# Getting started

## From a release

1. Download `spatial-browser-linux-x86_64.tar.gz` from [Releases](https://github.com/Loafer19/spatial-browser/releases).
2. Extract and run:

```sh
tar xzf spatial-browser-linux-x86_64.tar.gz
./compositor
```

`scripts/run.sh` (from a source checkout) wraps the binary and **auto-relaunches** after the known CEF SPA crash — see [[Limitations]].

## From source

Needs a Linux x86_64 machine, Rust toolchain, and a one-time CEF tools install:

```sh
cargo install cef --version 151.8.0+151.3.24 --locked \
  --root ~/.local/share/cargo-cef-tools
cargo install export-cef-dir --version 151.8.0+151.3.24 --locked \
  --root ~/.local/share/cargo-cef-tools
~/.local/share/cargo-cef-tools/bin/export-cef-dir --force ~/.local/share/cef

cargo build
./scripts/bundle.sh compositor
./scripts/run.sh compositor
./scripts/install.sh   # optional: desktop launcher entry
```

`CEF_PATH` defaults via `.cargo/config.toml` to `~/.local/share/cef`. Keep the installed `cef` / `export-cef-dir` version in sync with `crates/*/Cargo.toml` (also pinned in CI as `CEF_VERSION`).

## First minutes

| Action | Shortcut |
|--------|----------|
| New page (omnibox) | `Ctrl+T` |
| Bookmarks | `Ctrl+D` / `Ctrl+B` |
| History | `Ctrl+H` |
| Workspace slots | Top-edge chips / `Ctrl+1`…`9` / `Ctrl+N` |
| Settings | `Ctrl+,` |
| All shortcuts | `F1` or `Ctrl+/` |
| Userscripts | `Ctrl+Shift+U` |

More: [[Canvas-and-Shortcuts]], [[Features]].

## Import Chrome bookmarks

One-shot CLI (no ongoing sync):

```sh
cargo run -p import-chrome
```

Reads Chrome’s Bookmarks file and merges into `~/.config/spatial-browser/bookmarks.json`, keeping existing entries.
