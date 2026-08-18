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
- **Chrome UI** (new-page prompt, bookmarks/downloads/history lists,
  page switcher, help): no address bar — currently server-rendered
  `data:` HTML pages (built with plain Rust `format!`, see `pages/`),
  loaded as ordinary CEF pages rather than a native overlay. Each one
  signals back to the compositor by navigating to a fake
  `whatever://...` URL (`bookmark://`, `omnibox://`, `switcher://`,
  `download://`, `history://`), intercepted and canceled by
  `cef-bridge`'s `RequestHandler` — see `cef-bridge/src/navigation.rs`.
  An `egui` immediate-mode overlay in the same GPU context is the plan
  if/when the UI needs real interactivity beyond what an HTML form +
  custom-scheme link can do.
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
  - `hotkeys.rs` — canvas-level shortcuts, grouped into Pages / Lists /
    Canvas / Other (matches the F1 page's own grouping) — auto-layout,
    the page switcher, bookmarks/downloads/history, zoom, theme,
    back/forward — as opposed to input forwarded to the focused page.
  - `pages/` — generated `data:` HTML: the omnibox/new-page prompt,
    bookmarks/downloads/history lists, the Ctrl+K page switcher, F1 help.
  - `persistence/` — everything under `~/.config/spatial-browser/`:
    the canvas session, bookmarks, downloads, browsing history, and
    typed-omnibox input (`typed_history.rs` — deliberately not named
    `history.rs`, which is real visited-page history), each its own
    JSON file.
  - `input/`, `output/` — mouse/keyboard forwarding to CEF; wgpu surface,
    pipeline, themes.
  - `single_instance.rs` — refuses a second launch, focuses the running one.
- `crates/cef-bridge` — one file per CEF client-handler interface
  (`app`/`render`/`display`/`download`/`life_span`/`load`/
  `request_context`), plus `navigation` (the five custom-scheme
  interceptions above, all one `RequestHandler`) and `client` (ties
  every handler into the `cef::Client` CEF hands each spawned browser).

## Status

Working multi-page spatial canvas: pan/zoom (Ctrl+scroll, middle-drag or
Shift+drag), per-page drag (Alt+drag) and resize, z-order focus cycling
(Ctrl+Tab), zoom-to-canvas (Ctrl+Space), auto-layout into a grid
(Ctrl+G). Ctrl+T opens an omnibox page (`@prefix` shortcuts, e.g.
`@g`/`@y`, plus persisted typed-history) instead of a fixed new-tab URL;
Ctrl+Shift+T reopens the last closed page. Ctrl+K is a filterable
switcher over open pages — by search, not position: a page's spot in
z-order changes on every click, so there's no stable "page 3" the way a
browser tab bar has one. A page trying to open a link in a new
tab/window (`target="_blank"`, `window.open()`, middle-click) spawns a
regular canvas page instead of a native popup window outside the
canvas — see cef-bridge's `life_span.rs`.

Bookmarks (Ctrl+D / Ctrl+B) support favicons, inline rename, folders,
and delete. Downloads (Ctrl+J) land under `~/Downloads` with
collision-safe naming, no save-as dialog, a desktop notification on
completion, and a list to reopen or forget one. History (Ctrl+H) is
real visited-page tracking (distinct from the omnibox's typed-input
history), with a client-side toggle to regroup by day or by site and
local-time display computed in JS rather than Rust — see
`pages/history_list.rs`. F1 shows the full shortcut list, grouped the
same way as `hotkeys.rs`. Canvas layout, bookmarks, downloads, history,
and typed-omnibox input each persist to their own JSON file under
`~/.config/spatial-browser/` (debounced for the canvas, immediate for
the rest).

**Known limitation (unresolved):** CEF hard-crashes on SPA-style
client-side navigation (YouTube, Google Images lightbox) —
`TabInterface::GetFromContents` returns null inside
`ReadAnythingSoftNavigationObserver::OnSoftNavigation`, a Chromium-side
assumption that a Tab exists which doesn't hold for windowless/OSR
embeddings, and the crash happens in browser-process code reacting to a
renderer's IPC message — not a renderer crash — so there's no per-tab
process isolation to fall back on: one page hitting this takes down
every open page. Several command-line flags were tried (`cef-bridge`'s
`on_before_command_line_processing`, still in place as harmless
best-effort hardening) and confirmed to reach the process via
`chrome://version`, but none stop the crash. No newer `cef`/Chromium
version is available upstream to retry against. Mitigated rather than
fixed: `scripts/run.sh` auto-relaunches the bundled binary on
crash/non-zero exit (with a cooldown+giveup for repeated fast
failures), restoring the saved canvas — a crash costs at most the last
unsaved second of layout, not the session.

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
./scripts/run.sh compositor    # same, but auto-relaunches on crash —
                                # see Status's known-limitation note
```

## Releases

A pushed version tag (`git tag v0.2.0 && git push --tags`) triggers
`.github/workflows/release.yml`: builds, bundles, and attaches a
`spatial-browser-linux-x86_64.tar.gz` to a GitHub Release
(auto-downloading and caching the CEF binary distribution — no
`export-cef-dir` step needed in CI, same auto-fetch a fresh local build
does). Linux x86_64 only: this targets one desktop's CEF setup, not a
general cross-platform distribution.

## License

MIT — see [LICENSE](LICENSE).
