# Changelog

## Unreleased

- **Performance overlay** (`F3`, or "Performance overlay" in `Ctrl+K`): canvas
  frames/s, CPU time per frame (avg / max), CEF paints/s, tiles on / off screen,
  loading and parked in other workspaces, and resident memory of the app, its
  renderers, the GPU process and other helpers. Sampled once a second while shown.
- Command palette closes on a click outside it.
- Catppuccin Latte: white labels on accent buttons (WCAG AA contrast).
- Vault unlock: the heading's lock icon no longer fills the card.

## v0.10.0

Built-in pages overhaul: every `spatial://` page now uses one design system
(ui.css themed by the current theme, shared keyboard model, maud templates
that escape all data).

- **Design system** for all built-in pages; Catppuccin Latte (first light theme).
- **Command palette** (`Ctrl+K`): open pages, commands, bookmarks, history, typed URLs.
- **Settings**: new **Performance** tab — frame rate, GPU (integrated / discrete)
  and page painting (auto / GPU / CPU), the last two saved in `settings.json`
  and applied after restart (`SPATIAL_BROWSER_GPU` / `_OSR` still win); search
  across all tabs; custom search engine URL (validated http(s)).
- **Bookmarks manager**: folder tree with nested `A/B` folders, search, inline
  rename / move, multi-select delete and move, rename folder.
- **Start page** (`Ctrl+T`): most visited sites, bookmark tiles, recent input,
  suggestions as before.
- **History**, **Passwords**, **Scripts & styles**, **Downloads** (live progress),
  context menu, find bar, save prompt and workspaces ported to the new pages.
- New **Site info** and **About** pages (from the palette).
- Deletes in lists ask for a second `Delete` (no dialogs in OSR tiles).
- Security: `site://` actions, like every other page action, are accepted only
  from built-in tiles on their own token; site info reads local stores only and
  never shows passwords; the web `password://` autofill subset is unchanged.

## v0.9.0

Browser engine update, a security hardening pass across built-in pages and the
password vault, `spatial://` built-in pages, idle rendering, find-in-page,
local favicons and a much longer, searchable history.

**Engine**
- CEF 151.8 → **154.5.0+154.0.34** (cef-rs 154.5). The ReadAnything soft-navigation crash (CEF#4234) is fixed upstream.

**Security hardening**
- Built-in pages are served from **`spatial://page/<kind>/<token>`** instead of `data:` URLs. The token is issued to that one browser.
- `bookmark://`, `history://`, `settings://`, `password://` (vault UI) and the other actions are accepted only from those built-in tiles.
- Web pages may send only:
  - the `password://` autofill subset, from the main frame or the bridge's blank iframe, and bound to the main-frame origin;
  - `context://hit`, only after a real right-click.
- Autofill works only on HTTPS and on local-network or loopback HTTP.
- SOP/TLS bypass flags removed.
- Page fields are HTML/JS-escaped everywhere (escaping audit). Fixed an XSS through a `data:` page.
- Clipboard writes from pages need a gesture or Ctrl+C. `wl-copy` is fed through stdin. Paste is async.
- Popups are gated and opened as real windowless tiles. Risky downloads are quarantined.
- Queued script injections are skipped once the frame has moved to another origin.
- Config dir is private (0700). JSON configs are written atomically with backups, through a symlink-safe background writer. Browsing traces (history, downloads, typed URLs) keep no `.bak`.

**Password vault v2**
- Versioned file header with stored Argon2id parameters (64 MiB, t=3). The header is authenticated as AES-GCM AAD.
- v1 vaults are migrated on unlock.
- Idle auto-lock, zeroized secrets, atomic 0600 writes.
- The save-password prompt is its own compositor tile, outside the page.
- Generated passwords are copied as sensitive data.

**Pages, UI and features**
- **Find in page** (Ctrl+F) on CEF's native find. The bar is sized for HiDPI.
- **Favicons** for bookmarks, history, downloads and the switcher come from a local cache filled on visit. Lists never fetch `favicon.ico` from the listed sites.
- **History**:
  - page titles and short URLs;
  - kept for **90 days / up to 10 000 visits**, with automatic migration of the old file;
  - opens with one page of rows, then "Show more" and scroll loading;
  - search runs in Rust over the whole history;
  - "Recent" shows each URL once with a ×N visit count. #8, #10
- **Omnibox (Ctrl+T)** suggests from history and typed input. Host-prefix matches rank first. ↑/↓ to choose. #9
- Ctrl+Shift+1..9 moves the focused page to a workspace slot.
- Links can open in the system browser. Launch URLs are forwarded to the running instance.
- The context menu opens only for a real right-click.

**Performance**
- The app idles when nothing changes (frame pacer). Filter-engine rebuilds run off the render thread.
- The CPU paint texture is reused, and only dirty rects are uploaded.
- Tiles open from a pool of pre-started browsers.
- The first EasyList build finishes before pages are restored.

**Code and CI**
- `pending_actions` is split into a module per scheme. `vault`, `navigation` and `app` are split into submodules. Production `unwrap()`s are removed. #7
- CI runs `cargo fmt --check`, clippy `-D warnings` and tests. GitHub Actions moved to the Node 24 runtime. Tag releases no longer write caches.
- More tests: vault, CSV import, scheme parsers, history, suggestions.
- Crate versions now follow the release tag (0.9.0).

Linux x86_64 only. See the [wiki](https://github.com/Loafer19/spatial-browser/wiki) for configuration and limitations.

**Full Changelog**: https://github.com/Loafer19/spatial-browser/compare/v0.8.0...v0.9.0
