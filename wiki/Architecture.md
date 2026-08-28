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

There is **no native chrome** (no address bar widgets). Utility UI is server-rendered `data:` HTML loaded as ordinary CEF pages:

- Omnibox, bookmarks/history/downloads/workspaces/settings/userscripts lists, page switcher, F1 help
- Actions signal back via fake URLs (`bookmark://…`, `settings://…`, `userscripts://…`, …)
- `cef-bridge`’s `RequestHandler::on_before_browse` cancels those navigations and queues actions for the compositor

## Rendering

- Each canvas page owns a CEF browser + wgpu texture
- Default path (v0.5+): **GPU shared-texture OSR** (`accelerated_osr`)
- Fallback: CPU `on_paint` memcpy — set `SPATIAL_BROWSER_OSR=cpu`
- Hybrid GPUs: wgpu prefers LowPower/iGPU so DMA-BUF import matches CEF — see [[Configuration]]

## Profile

All pages share one CEF profile under `~/.config/spatial-browser/cef_data` — cookies and logins persist across tiles and restarts.
