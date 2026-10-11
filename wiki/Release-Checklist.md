# Release checklist

Manual pass on a real desktop before tagging. Build the bundle (`CARGO_BUILD_JOBS=2 ./scripts/bundle.sh compositor`) and run it with `./scripts/run.sh compositor`. Tick every box. Note anything odd in the release PR.

## 0. Start

- [ ] Clean start with an existing profile: the session/workspace restores and **only visible tiles load at first**. Off-screen tiles load when you pan to them.
- [ ] First run with an empty `~/.config/spatial-browser` (rename the folder): a single Google tile, no errors in the terminal.
- [ ] `restarts.log` gets no new lines during the whole pass.

## 1. Start page / omnibox (`Ctrl+T`)

- [ ] A typed URL opens (`example.com`, `https://…`, `localhost:8080`).
- [ ] Search text uses the default engine. `@y`, `@ddg`, `@wiki`, `@bing` and `@g` prefixes work.
- [ ] Suggestions appear while typing. `↑`/`↓` select, `Enter` opens the selection or your text, `Esc` clears the selection.
- [ ] Most visited / Bookmarks tiles and Recent chips open on click and with arrows + `Enter`.

## 2. Built-in pages (in **each** theme: Tokyo Night, ANSI Terminal, Catppuccin Latte)

- [ ] **Palette** `Ctrl+K`: commands, open pages, bookmarks, history. Site info and About open.
- [ ] **Bookmarks** `Ctrl+B`:
  - [ ] Folder tree filter, Unfiled.
  - [ ] `r` rename/move, `Space`/`a` select, double `Del` deletes the selection, Move, Rename folder.
  - [ ] Import… from a Chrome profile.
- [ ] **History** `Ctrl+H`:
  - [ ] Search covers everything. Show more / scroll loads more.
  - [ ] Recent ×N, By day, By site.
  - [ ] `Del` removes. On a ×N row it needs a second `Del`. Clear all.
- [ ] **Downloads** `Ctrl+J`: live progress, pause/resume/cancel. Opening an executable reveals the folder instead of running it.
- [ ] **Settings** `Ctrl+,`:
  - [ ] Search finds settings in other tabs. Every toggle sticks after reopening.
  - [ ] Custom search engine: `javascript:` is rejected.
  - [ ] Frame rate applies live.
- [ ] **Scripts & styles** `Ctrl+Shift+U`: toggle, Reload, open folders. A `spatial-ui` style applies to built-in pages.
- [ ] **Workspaces** `Ctrl+Shift+W`: switch, rename, delete. `Ctrl+1–9`, `Ctrl+N`.
- [ ] **Help** `F1`, **About** (versions, GPU, paint path), **Site info** (HTTPS and HTTP page, Forget history, bookmark toggle).
- [ ] Context menu (right-click page / link / image / password field), find bar `Ctrl+F` (next/prev, count, Esc).
- [ ] `?` shows the keys overlay on every list page. `/` focuses search, `Esc` clears / closes.

## 3. Vault and autofill

- [ ] Create the vault (mismatch / short password errors), lock `Ctrl+Shift+L`, unlock (a wrong password shows an error).
- [ ] Add an entry, import Chrome/Bitwarden CSV, generator + Copy, Never save list.
- [ ] HTTPS login form: a fill suggestion appears on focus and fills. A plain-HTTP public site gets **no** suggestion. A local HTTP (`192.168.x`, `localhost`) site does.
- [ ] Save prompt after login: Save / Update / Never.
- [ ] After 15 min idle the vault locks and the passwords list swaps to the unlock form.
- [ ] A web page navigating to `bookmark://`, `settings://`, `history://`, `site://` or `spatial://` does nothing (check the warn lines in the log).

## 4. Popups, downloads, misc

- [ ] `window.open` from a click opens a tile. Popups without a user gesture are blocked (logged).
- [ ] A download lands in `~/Downloads` with a desktop notification. The file is not executable.
- [ ] Reader mode `Ctrl+Shift+R`, open in system browser `Ctrl+Shift+O`, copy URL `Ctrl+Shift+C`, zoom, back/forward.
- [ ] Clean URLs strips `utm_*`. Ad blocking on a known ad-heavy page, toggling lists in Settings.

## 5. GPU / OSR modes

Run each and scroll a video page (YouTube) plus a long article:

- [ ] Default (Settings → Performance: Integrated + Auto). About shows "GPU shared textures".
- [ ] `SPATIAL_BROWSER_OSR=cpu`: About shows "CPU copy" and Settings shows "Pinned by".
- [ ] Settings → GPU: Discrete → the restart banner shows. After restart About shows Discrete + CPU copy (Auto).
- [ ] `SPATIAL_BROWSER_GPU=high SPATIAL_BROWSER_OSR=gpu` (expect black tiles on hybrid laptops. It must not crash.)
- [ ] Crash recovery: `kill -9` a renderer (`ps -ef | grep type=renderer`). Only that tile reloads. Kill it 4× and it stops auto-reloading; `Ctrl+R` brings it back.
- [ ] Kill the GPU process (`type=gpu-process`) a few times. The app survives.

## 6. Release mechanics

- [ ] `cargo fmt`/`clippy`/`test -j 2` green. CI green on master.
- [ ] Versions bumped, `CHANGELOG.md` section moved from Unreleased, wiki updated (synced by the `wiki-sync` workflow).
