# Getting started

## From a release

1. Download `spatial-browser-linux-x86_64.tar.gz` from [Releases](https://github.com/Loafer19/spatial-browser/releases).
2. Extract and run:

```sh
tar xzf spatial-browser-linux-x86_64.tar.gz
./compositor
```

`scripts/run.sh` (from a source checkout) wraps the binary and **auto-relaunches** after the known CEF SPA crash — see [[Limitations]]. Restarts are appended to `~/.config/spatial-browser/restarts.log`.

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

Set `CEF_PATH` (usually `~/.local/share/cef`): copy `.cargo/config.toml.example` → `.cargo/config.toml` (gitignored) or export the env var. Keep `cef` / `export-cef-dir` in sync with `crates/*/Cargo.toml` (also pinned in CI as `CEF_VERSION`).

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

## Import browser bookmarks

**Ctrl+B** → **Import…** → file dialog (always). If a profile is found, it opens in that folder (e.g. `~/.config/google-chrome/Default/`) with `Bookmarks` or `AccountBookmarks` preselected. Merges by URL; existing entries are kept.
