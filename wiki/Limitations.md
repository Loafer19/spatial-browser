# Limitations

Known, documented constraints — not a wishlist.

## No Chrome Web Store / extensions

CEF **windowless / Alloy-style OSR** does not expose extension loading. Upstream: extensions need Chrome-style UI windows; Alloy OSR does not get `chrome://extensions` or a supported `load_extension` path.

**Substitute:** [[Userscripts]] (content-script-like inject + small GM API).

Long-term hope for embedders: Igalia’s `//content` + `//extensions` work — not available as a turnkey CEF API yet.

## SPA soft-navigation crash

Chromium bug: `ReadAnythingSoftNavigationObserver` assumes a real browser tab. Windowless WebContents can null-deref on client-side navigations (YouTube, Google Images lightbox, …).

- Fixed upstream in M152; this project is on **CEF 154.x**
- `disable-features=ImmersiveReadAnything` stays on in `cef-bridge` as belt-and-braces (OSR has no reading-mode UI)
- Still: some crashes happen — `scripts/run.sh` **auto-relaunches** and restores the saved canvas (debounced ~1s). Unlike Chrome’s per-tab process isolation, a crash takes down the whole app
- Each relaunch appends a line to `~/.config/spatial-browser/restarts.log` (`reason=cef_soft_nav` vs `fast_crash`, exit code, elapsed, counts) so CEF/VAAPI changes can be judged from restart rate

## Clipboard

CEF’s own clipboard does not work in this OSR embedding. Copy/paste are reimplemented (`wl-copy` / `wl-paste` + injected bridge / `Ctrl+V` intercept).

## Find-in-page

`Ctrl+F` uses CEF's native find (highlights, match count, Enter / Shift+Enter, match case). The bar is a small tile above the searched page; it searches the page that was topmost when opened.

## Missing “browser chrome” features

Password manager is **local-only** (see [[Password-Manager]]); CSV import from Chrome/Bitwarden works, but no cloud sync, TOTP, or passkeys yet. No DevTools panel in the canvas, no built-in PDF chrome beyond what CEF gives, no full-text history search.

## Platform

**Linux x86_64 only.**

## No H.264 / AAC (Twitch, many MP4 streams)

Official CEF binaries used here are **`minimal`** and ship **without proprietary codecs**. Chromium logs:

`Proprietary codecs not enabled in this Chromium build`

Twitch live video typically needs **H.264 + AAC**, so the player surfaces **Error #4000** (“not supported in this browser”). GPU choice / VAAPI / adblock do not fix this — the codecs were never compiled in. Hardware VAAPI decode is disabled in CEF flags to avoid `vaEndPicture` spam on hybrid GPUs; that does not restore Twitch playback.

Open codecs that *are* present (VP8/VP9/AV1) do not help when the site only offers H.264.

**Workaround:** `Ctrl+Shift+O` (or right-click → *Open page / link in system browser*) hands the URL to the desktop's default browser via `xdg-open`. Enabling codecs requires a **custom CEF/Chromium build** (`proprietary_codecs=true`, `ffmpeg_branding=Chrome`) plus awareness of patent/licensing constraints when redistributing. PyPI Qt WebEngine wheels have the same codec gap unless the distro build includes openh264.

## Security model

Child processes run with Chromium's default same-origin policy and TLS checks (no `disable-web-security` / `ignore-certificate-errors`). Built-in pages talk to the compositor via fake `scheme://` navigations; only ephemeral tiles on a `spatial://page/…` URL whose random token the compositor issued to that very browser may send them; web pages can't navigate to, link or frame `spatial://`. Web pages get just the autofill subset (`password://` query/fill/save-offer from the main frame or the bridge's blank iframe, bound to the page's own main-frame origin) and `context://hit` right after a real right-click. Saving a password is only possible from the browser-owned prompt tile. Still a personal tool — no per-site process isolation guarantees beyond Chromium's defaults, no sandbox hardening review.
