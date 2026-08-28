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

## Workspaces (live slots)

Numbered **live slots** (default three), not named one-shot snapshots.

| Action | How |
|--------|-----|
| Switch slot | Top-edge chip strip, or `Ctrl+1`…`Ctrl+9` |
| New slot | Chip `+`, `Ctrl+N`, or context menu **New workspace** |
| Slot list / delete | `Ctrl+Shift+W` |

**Behavior**

- Always one active slot. Switching **autosaves** the current canvas (URLs, rects, viewport, theme) into that slot, then shows the target.
- Slots start **lazy**: first visit is an empty canvas until you open pages there.
- Within one process run, pages of a slot you’ve already opened stay **parked in memory** when you leave — switching back restores the same CEF browsers (no reload). A cold start still restores from the on-disk snapshot.
- File: `~/.config/spatial-browser/workspaces.json` (version 2). Older v1 named snapshots migrate into visited slots on load.

Hover the **top edge** of the window to reveal the chip strip (auto-hides when the pointer leaves).

## Reader mode (`Ctrl+Shift+R`)

Heuristic article extract → single-column view (Light / Sepia / Dark from Settings). Toggle off **reloads** the page (DOM was replaced in place).

## Ad / tracker blocking

Settings → **Blocking** tab (`Ctrl+,`):

- **Content filtering** master switch
- **Filter lists** (subscription rows): Peter Lowe hosts; **EasyList** / **EasyPrivacy** (Brave `adblock` network engine)
- **Custom hosts** add/remove
- **Advanced**: network / cosmetic / scriptlets — all three live (scriptlets **Off** by default)

**Network:** EasyList-syntax rules cancel matching resource requests (plus optional Peter Lowe host list + custom hosts).

**Cosmetic:** on document-start, injects `{display:none!important}` for URL-specific hide selectors.

**Scriptlets:** optional `##+js(...)` inject from a vendored classic uBO `scriptlets.js` (experimental; can break sites). Generic class/id cosmetic follow-up still ahead.

List files: `~/.config/spatial-browser/filters/` (`easylist.txt`, `easyprivacy.txt`, `scriptlets.js` — seeded from the bundle on first run).

## Clean URLs

Strips tracking params from navigations (`utm_*`, `fbclid`, `gclid`, …). Separate from ad-block; toggle in Settings.

## Themes

Tokyo Night / ANSI Terminal for canvas chrome. Cycle with `Ctrl+Shift+Space`, or pick in Settings. Reader themes are separate.

## Frame rate

Settings: 60 / 90 / 120 target. Monitor refresh is still the real ceiling.

## Scripts & styles (`Ctrl+Shift+U`)

Userscripts (`.js`) and userstyles (`.css`). Built-in UI pages can be styled with `@match spatial-ui` — see [[Userscripts]].

## Passwords (`Ctrl+Shift+P`)

Encrypted local vault, fill suggestions on focus, Chrome/Bitwarden CSV import, save prompts, generator, never-save list — see [[Password-Manager]].

## Single instance

Second launch focuses the existing window (Unix socket under the config dir) instead of starting a second CEF profile.
