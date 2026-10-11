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
