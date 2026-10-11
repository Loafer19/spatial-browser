# Architecture

## Stack

```
┌─────────────────────────────────────────┐
│  compositor (winit window + wgpu)       │
│  canvas of page quads, hotkeys, lists   │
└─────────────────┬───────────────────────┘
                  │ spawn / paint / input
┌─────────────────▼───────────────────────┐
│  cef-bridge                             │
│  CEF client handlers (OSR, navigation,  │
│  downloads, load, blocklist, …)         │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│  CEF / Chromium (windowless Alloy-style)│
│  one persistent profile (cookies/login) │
└─────────────────────────────────────────┘
```

## Crates

| Crate | Role |
|-------|------|
| `compositor` | Window, wgpu surface, session/canvas, pages UI, input, persistence |
| `cef-bridge` | One file per CEF handler + custom-scheme interception |

## UI model

There is **no native chrome** (no address bar widgets). Utility UI is HTML the compositor renders and serves from memory under a registered `spatial://page/<kind>/<token>` scheme (no `data:` URLs, no URL-length limit; list actions refresh the tile in place), loaded as ordinary CEF pages:

- Omnibox, bookmarks/history/downloads/workspaces/settings/userscripts lists, page switcher, F1 help
- Actions signal back via fake URLs (`bookmark://…`, `settings://…`, `userscripts://…`, …)
- `cef-bridge`’s `RequestHandler::on_before_browse` (`navigation/`) cancels those navigations and queues actions for the compositor (`pending_actions/`, one module per scheme)
- Actions are honoured only from an ephemeral tile whose `spatial://` token was issued to that browser; web pages get only the `password://` autofill subset and `context://hit`
- Data a page asks for (history search pages, omnibox suggestions, find counts) comes back by `execute_java_script` into that same built-in tile only — never into web content

## Rendering

- Each canvas page owns a CEF browser + wgpu texture
- Default path (v0.5+): **GPU shared-texture OSR** (`accelerated_osr`)
- Fallback: CPU `on_paint` memcpy — set `SPATIAL_BROWSER_OSR=cpu`
- Hybrid GPUs: wgpu prefers LowPower/iGPU so DMA-BUF import matches CEF — see [[Configuration]]

## Tiles

- New tiles come from a warm pool of pre-started hidden browsers (built-in / web), refilled ~2 s after use
- Allowed popups are created by CEF as windowless children of their opener and adopted as tiles
- Rendering is paced: an idle canvas draws no frames; filter-engine rebuilds run off the render thread; CPU paints upload only dirty rects

## Persistence

`~/.config/spatial-browser/` (0700): session, workspaces, bookmarks, history (v2, 90 days / 10k), typed history, settings, downloads, favicons, `vault.enc`. JSON is written atomically by a background writer (newest version wins); settings-like files keep a `.bak`, browsing traces don't.

## Profile

All pages share one CEF profile under `~/.config/spatial-browser/cef_data` — cookies and logins persist across tiles and restarts.

## Crash resilience

What CEF 154 (Chromium 154) isolates, and what the compositor adds:

- **Renderer processes** are per site (site isolation is on by default). A crashing or OOM-killed renderer (e.g. a heavy YouTube tab) takes only its own tiles down; CEF reports it via `CefRequestHandler::OnRenderProcessTerminated`. cef-bridge queues it (`PENDING_RENDER_CRASHES`); the compositor reloads **only that tile**, with backoff 1 s → 5 s → 30 s, and stops after 3 crashes in 10 min (reload by hand with `Ctrl+R`). Built-in tiles are not auto-reloaded.
- **GPU process**: Chromium relaunches it after a crash, but after a few crashes it exits the whole *browser* process ("GPU process isn't usable. Goodbye."). We pass `--disable-gpu-process-crash-limit` so it keeps relaunching (falling back to software GL) instead. Our own compositor (wgpu) is independent of the Chromium GPU process; with `osr = gpu` shared textures a GPU-process restart only means a few missed frames.
- **Browser process** crashes (e.g. the old SPA soft-nav null-deref) still end the app; `scripts/run.sh` relaunches it and logs to `restarts.log`.
- **Restore without a storm**: restored tiles (session / workspace slot) start as blank browsers and load their URL only when on screen, topmost first, one load start per 150 ms. Off-screen tiles load when panned into view. Saves keep the real URL of tiles that haven't loaded yet.
- Not available / not done: CEF has no per-tile process limit or memory cap API beyond Chromium switches (`--renderer-process-limit` would *merge* sites into shared processes, which weakens isolation, so it is not set). `OnRenderProcessUnresponsive` is left at the default (keep waiting).
