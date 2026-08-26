# Userscripts

Real Chrome/WebExtensions are **not** available on this CEF OSR embedding (see [[Limitations]]). Instead: a Tampermonkey-style runner.

## Install a script

1. Put a `.js` file in `~/.config/spatial-browser/userscripts/`
2. Open **Ctrl+Shift+U** → **Reload** (or just open the list — it reloads from disk)

## Metadata

```js
// @name   Greyscale reddit
// @match  *://*.reddit.com/*
// @exclude *://*.reddit.com/r/all/*
// @run-at document-end

(function () {
  document.body.style.filter = 'grayscale(1)';
})();
```

| Tag | Meaning |
|-----|---------|
| `@name` | Display name in the list (default: filename) |
| `@match` | Wildcard glob against the full URL (required; any of N matches) |
| `@exclude` | Same glob language; if any hits, script does not run |
| `@run-at` | `document-start`, `document-end` (default), or `document-idle` |

Match patterns are plain `*` globs, not the full WebExtension parser. `*.example.com` also matches the apex host (e.g. `https://github.com/...`).

## GM_* prelude

Injected before your code:

- `GM_addStyle(css)`
- `GM_getValue(key, default)`
- `GM_setValue(key, value)` — backed by `localStorage` keys prefixed `gm:`

Enough for many GreasyFork scripts; not a full Violentmonkey/Tampermonkey API (`GM_xmlhttpRequest`, `@grant`, etc. are absent).

## List UI (Ctrl+Shift+U)

- Toggle enable/disable (persisted in `userscripts_state.json`, file stays on disk)
- **Reload** — re-read the directory without restarting the browser
- **Open folder** — `xdg-open` on the userscripts directory

Disabled scripts are skipped at inject time.
