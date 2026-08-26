# Userscripts & userstyles

Real Chrome/WebExtensions are **not** available on this CEF OSR embedding (see [[Limitations]]). Instead: Tampermonkey-style scripts and Stylus-style CSS.

Open **Ctrl+Shift+U** for the combined list (Reload re-reads both folders).

---

## Userscripts (`.js`)

### Install

1. Put a `.js` file in `~/.config/spatial-browser/userscripts/`
2. **Ctrl+Shift+U** → **Reload**

### Metadata

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
| `@name` | Display name (default: filename) |
| `@match` | URL glob **or** `spatial-ui` / `spatial:*` for built-in chrome pages |
| `@exclude` | Same language; any hit skips the script |
| `@run-at` | `document-start`, `document-end` (default), `document-idle` |

```js
// @name   Tweak settings list
// @match  spatial-ui
// @run-at document-end

document.body.style.letterSpacing = '0.02em';
```

### GM_* prelude

- `GM_addStyle(css)`
- `GM_getValue(key, default)` / `GM_setValue(key, value)` (`localStorage`, `gm:` prefix)

---

## Userstyles (`.css`)

### Install

1. Put a `.css` file in `~/.config/spatial-browser/userstyles/`
2. **Ctrl+Shift+U** → **Reload**

### Metadata

```css
/* @name   Dim built-in lists */
/* @match  spatial-ui */

body {
  filter: brightness(0.92);
}
```

```css
/* @name   Example.com denser */
/* @match  *://*.example.com/* */

article { max-width: 40rem; margin: 0 auto; }
```

| Tag | Meaning |
|-----|---------|
| `@name` | Display name |
| `@match` | URL glob **or** `spatial-ui` / `spatial:*` for built-in chrome pages |
| `@exclude` | Same language |

### Built-in / “default” pages

Omnibox, bookmarks, history, downloads, workspaces, settings, help, switcher, and this scripts list are ephemeral `data:` HTML pages. Their real URLs are huge, so site globs never match them.

For **both** scripts and styles use:

```
@match spatial-ui
```

(`spatial:*` is an alias.) That matches **any** built-in UI page. You can also `@match data:*` for a raw `data:` glob; `spatial-ui` is the supported shortcut.

---

## List UI (Ctrl+Shift+U)

- Toggle scripts / styles (state in `userscripts_state.json` / `userstyles_state.json`)
- **Reload** — both directories
- **Scripts folder** / **Styles folder** — `xdg-open`
