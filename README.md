# spatial-browser

Custom browser shell: own UI, own history/bookmarks, plugin API, and a
spatial canvas for arranging pages instead of flat tabs.

## Architecture

- **Web engine:** CEF (Chromium Embedded Framework), off-screen rendering
  mode with shared GPU textures (`OnAcceleratedPaint`) — each page renders
  straight into a GPU texture, no CPU copy per frame.
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

- `crates/compositor` — window, wgpu surface, canvas renderer, egui chrome.
- `crates/cef-bridge` — CEF FFI bindings and the OSR/shared-texture plumbing.

## Status

Bootstrap only. Neither crate has real CEF or wgpu wiring yet.

## Build

```
cargo build
```
