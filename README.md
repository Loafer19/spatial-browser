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
- **Chrome UI** (new-page prompt, bookmarks list, help): no address bar —
  currently server-rendered `data:` HTML pages (built with plain Rust
  `format!`, see `pages/`), loaded as ordinary CEF pages rather than a
  native overlay. An `egui` immediate-mode overlay in the same GPU
  context is the plan if/when the UI needs real interactivity beyond
  what an HTML form + custom-scheme link can do.
- **Plugins:** planned, not started — native plugin host (JS bundle
  loaded into a hidden CEF browser instance) talking to the native side
  over CEF IPC. Chrome extension compatibility (content scripts,
  `chrome.storage`, etc.) would be a secondary, best-effort layer — no
  chrome.* API covers spatial layout, so that stays a native-only
  surface.
- **Memory:** renderer processes for pages outside the visible viewport
  get suspended/discarded; last frame is kept as a GPU-side thumbnail
  texture until the page scrolls back into view.

## Layout

- `crates/compositor` — window, wgpu surface, canvas renderer, CEF process
  bootstrap and event loop.
  - `main.rs` — CEF multi-process bootstrap, winit event loop.
  - `app.rs` — `ApplicationHandler`: input routing, drag/resize/pan/zoom,
    the redraw loop (polls pending bookmark/omnibox actions, persists,
    renders).
  - `session.rs` — canvas state (pages, viewport, theme, closed-page
    undo stack), each mutation marking itself dirty for persistence.
  - `viewport.rs` — world-space <-> screen-space pan/zoom mapping.
  - `browser.rs` — `Page`: spawns a CEF OSR browser into a rect,
    owns its GPU texture.
  - `hotkeys.rs` — canvas-level shortcuts (new/close/reopen page,
    bookmarks, help, zoom, theme, back/forward) as opposed to input
    forwarded to the focused page.
  - `pages/` — generated `data:` HTML for the omnibox/new-page prompt,
    bookmarks list, and F1 help.
  - `persistence/` — everything under `~/.config/spatial-browser/`:
    the canvas session, bookmarks, and typed-omnibox history, each its
    own JSON file.
  - `input/`, `output/` — mouse/keyboard forwarding to CEF; wgpu surface,
    pipeline, themes.
  - `single_instance.rs` — refuses a second launch, focuses the running one.
- `crates/cef-bridge` — CEF wrapper types (App/BrowserProcessHandler/
  RenderHandler/Client), both OSR paint paths, and the custom-scheme
  (`bookmark://`, `omnibox://`) request interception used to get clicks
  in a generated page back to the compositor.

## Status

Working multi-page spatial canvas: pan/zoom (Ctrl+scroll, middle-drag or
Shift+drag), per-page drag (Alt+drag) and resize, z-order focus cycling
(Tab), zoom-to-canvas (Ctrl+Space). Ctrl+T opens an omnibox page
(`@prefix` shortcuts, e.g. `@g`/`@y`, plus persisted typed-history) instead
of a fixed new-tab URL; Ctrl+Shift+T reopens the last closed page.
Bookmarks (Ctrl+D / Ctrl+B) support favicons, inline rename, folders, and
delete. F1 shows a shortcut cheat sheet. Canvas layout, bookmarks, and
history each persist to their own JSON file under
`~/.config/spatial-browser/` (debounced for the canvas, immediate for the
other two).

**Known limitation (unresolved):** CEF hard-crashes on SPA-style
client-side navigation (YouTube, Google Images lightbox) —
`TabInterface::GetFromContents` returns null inside
`ReadAnythingSoftNavigationObserver::OnSoftNavigation`, a Chromium-side
assumption that a Tab exists which doesn't hold for windowless/OSR
embeddings. Several command-line flags were tried (`cef-bridge`'s
`on_before_command_line_processing`, still in place as harmless
best-effort hardening) and confirmed to reach the process via
`chrome://version`, but none stop the crash. No newer `cef`/Chromium
version is available upstream to retry against. Accepted as a known
limitation rather than chased further.

Rendering via CEF's CPU OSR path (`on_paint` → `wgpu::Queue::write_texture`),
not the GPU shared-texture path yet.

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
