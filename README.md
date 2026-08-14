# spatial-browser

Custom browser shell: own UI, own history/bookmarks, plugin API, and a
spatial canvas for arranging pages instead of flat tabs.

## Architecture

- **Web engine:** CEF (Chromium Embedded Framework), off-screen rendering.
  Design target is the shared-GPU-texture path (`on_accelerated_paint`,
  zero CPU copy per frame); currently running the CPU path (`on_paint`,
  one `write_texture` copy per frame) — see Status.
- **Compositor:** `wgpu`, draws pages as textured quads on a pannable/
  zoomable canvas. Instanced rendering keeps FPS high with many pages open.
- **Chrome UI** (url bar, history, bookmarks, plugin panels): `egui`,
  immediate-mode, rendered in the same GPU context as the compositor.
- **Plugins:** native plugin host (JS bundle loaded into a hidden CEF
  browser instance) talking to the native side over CEF IPC. Chrome
  extension compatibility (content scripts, `chrome.storage`, etc.) is a
  secondary, best-effort layer — no chrome.* API covers spatial layout,
  so that stays a native-only surface.
- **Memory:** renderer processes for pages outside the visible viewport
  get suspended/discarded; last frame is kept as a GPU-side thumbnail
  texture until the page scrolls back into view.

## Layout

- `crates/compositor` — window, wgpu surface, canvas renderer, CEF process
  bootstrap and event loop.
- `crates/cef-bridge` — CEF wrapper types (App/BrowserProcessHandler/
  RenderHandler/Client) and both OSR paint paths.

## Status

One page (`example.com`), one full-window quad — proves the pipeline
before multi-page spatial layout. Rendering via CEF's CPU OSR path
(`on_paint` → `wgpu::Queue::write_texture`), not the GPU shared-texture
path yet.

**Why not the shared-texture path:** on this dev machine (hybrid
NVIDIA + AMD laptop graphics), CEF's GPU process renders on whichever GPU
is display-driving (the AMD iGPU here), while wgpu's `HighPerformance`
adapter pick can land on the discrete GPU — the DMA-BUF CEF exports then
fails to import cross-device (`vkAllocateMemory` →
`ERROR_OUT_OF_DEVICE_MEMORY`). Chromium also sanitizes environment
variables passed to its child processes, so there's no reliable way from
the launching process to force both onto the same physical GPU. Revisit
by either pinning both to the same adapter via system-level GPU-offload
config, or by explicitly selecting a specific `wgpu::Adapter` (enumerate
+ match by name) instead of trusting `PowerPreference`.

Both OSR paths are wired up in `cef-bridge` behind
`WindowInfo::shared_texture_enabled` in `compositor/src/main.rs:resumed`
— flipping that bool (plus the `accelerated_osr` feature) is the whole
switch once the GPU-selection problem above is solved.

## Build

Machine-specific one-time setup:

```sh
# Cache the CEF binary distribution (also happens automatically on first
# build, just slower — this pre-warms it):
cargo run -p export-cef-dir -- --force ~/.local/share/cef
# (needs the cef-rs workspace checked out separately; see
# https://github.com/tauri-apps/cef-rs)

# bundle-cef-app, used by scripts/bundle.sh:
cargo install cef --version 151.4.0+151.3.17 --locked \
  --root ~/.local/share/cargo-cef-tools
```

`CEF_PATH` and the linker `rpath` are set in `.cargo/config.toml` — plain
`cargo build`/`cargo check` work with no manual env vars. Update
`CEF_PATH` there if `export-cef-dir`'s output directory differs.

```sh
cargo build                    # compiles, doesn't need CEF running
./scripts/bundle.sh compositor # bundles + the CEF subprocess helper
target/bundle/compositor       # only the bundled binary runs (CEF's
                                # multi-process model needs the helper)
```
