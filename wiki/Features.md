# Features

## Omnibox (`Ctrl+T`)

Type a URL or search. Prefixes: `@g`, `@y`, `@ddg`, `@bing`, `@wiki`. Typed history is remembered. Default search engine is chosen in Settings.

## Bookmarks / history / downloads

| UI | Shortcut | Notes |
|----|----------|--------|
| Bookmark current | `Ctrl+D` | |
| Bookmarks list | `Ctrl+B` | Folders, rename, delete |
| History | `Ctrl+H` | Day/site grouping |
| Downloads | `Ctrl+J` | To `~/Downloads`, desktop notification on complete |

Chrome bookmarks: one-time merge via `import-chrome` — [[Getting-Started]].

## Workspaces (`Ctrl+Shift+W`)

Named **save snapshots** of the whole canvas (URLs, rects, viewport, theme). Load closes everything open and restores the snapshot — not a live auto-synced workspace.

## Reader mode (`Ctrl+Shift+R`)

Heuristic article extract → single-column view (Light / Sepia / Dark from Settings). Toggle off **reloads** the page (DOM was replaced in place).

## Ad / tracker blocking

- Peter Lowe’s ad-server list (~3500 domains), request-level block
- On by default; toggle + custom hosts in Settings (`Ctrl+,`)

## Clean URLs

Strips tracking params from navigations (`utm_*`, `fbclid`, `gclid`, …). Toggle in Settings.

## Themes

Tokyo Night / ANSI Terminal for canvas chrome. Cycle with `Ctrl+Shift+Space`, or pick in Settings. Reader themes are separate.

## Frame rate

Settings: 60 / 90 / 120 target. Monitor refresh is still the real ceiling.

## Single instance

Second launch focuses the existing window (Unix socket under the config dir) instead of starting a second CEF profile.
