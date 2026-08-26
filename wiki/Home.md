# spatial-browser

Personal browser shell: pages as pannable/zoomable tiles on a canvas instead of flat tabs.

**Engine:** CEF (Chromium Embedded Framework), off-screen rendering  
**Compositor:** `wgpu` textured quads  
**Platform:** Linux x86_64

## Quick start

```sh
# From a release tarball
tar xzf spatial-browser-linux-x86_64.tar.gz
./compositor

# Or from source — see [[Getting-Started]]
./scripts/bundle.sh compositor
./scripts/run.sh compositor
```

Latest release: [v0.5.0](https://github.com/Loafer19/spatial-browser/releases/tag/v0.5.0)

## Wiki map

| Page | What’s on it |
|------|----------------|
| [[Getting-Started]] | Install, run, Chrome bookmark import |
| [[Architecture]] | Crates, CEF OSR, canvas UI model |
| [[Canvas-and-Shortcuts]] | Pan/zoom/touch + every hotkey |
| [[Userscripts]] | Scripts + userstyles (`spatial-ui` for built-in pages) |
| [[Password-Manager]] | Local vault, autofill, save prompts (`Ctrl+Shift+P`) |
| [[Features]] | Bookmarks, history, workspaces, reader, adblock, … |
| [[Configuration]] | Config paths, settings, env overrides |
| [[Limitations]] | No Chrome Web Store, known CEF crashes, workarounds |
| [[Building-and-Releasing]] | CEF pin, bundle, version tags |

## Not in this wiki

Deep Rust module docs — follow the code comments and [README](https://github.com/Loafer19/spatial-browser#readme). This wiki is for *using* and *understanding* the product, not duplicating the source tree.
