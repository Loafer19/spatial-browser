# Limitations

Known, documented constraints — not a wishlist.

## No Chrome Web Store / extensions

CEF **windowless / Alloy-style OSR** does not expose extension loading. Upstream: extensions need Chrome-style UI windows; Alloy OSR does not get `chrome://extensions` or a supported `load_extension` path.

**Substitute:** [[Userscripts]] (content-script-like inject + small GM API).

Long-term hope for embedders: Igalia’s `//content` + `//extensions` work — not available as a turnkey CEF API yet.

## SPA soft-navigation crash

Chromium bug: `ReadAnythingSoftNavigationObserver` assumes a real browser tab. Windowless WebContents can null-deref on client-side navigations (YouTube, Google Images lightbox, …).

- Upstream fix targeted M152; this project is on **CEF 151.x** until cef-rs publishes 152
- Mitigation: `disable-features=ImmersiveReadAnything` in `cef-bridge`
- Still: some crashes happen — `scripts/run.sh` **auto-relaunches** and restores the saved canvas (debounced ~1s). Unlike Chrome’s per-tab process isolation, a crash takes down the whole app

## Clipboard

CEF’s own clipboard does not work in this OSR embedding. Copy/paste are reimplemented (`wl-copy` / `wl-paste` + injected bridge / `Ctrl+V` intercept).

## Find-in-page

`Ctrl+F` was built once, reverted as too buggy — not currently supported.

## Missing “browser chrome” features

Password manager is **local-only** (see [[Password-Manager]]); CSV import from Chrome/Bitwarden works, but no cloud sync, TOTP, or passkeys yet. No DevTools panel in the canvas, no built-in PDF chrome beyond what CEF gives, no full-text history search.

## Platform

**Linux x86_64 only.**

## No H.264 / AAC (Twitch, many MP4 streams)

Official CEF binaries used here are **`minimal`** and ship **without proprietary codecs**. Chromium logs:

`Proprietary codecs not enabled in this Chromium build`

Twitch live video typically needs **H.264 + AAC**, so the player surfaces **Error #4000** (“not supported in this browser”). GPU choice / VAAPI / adblock do not fix this — the codecs were never compiled in. Hardware VAAPI decode is disabled in CEF flags to avoid `vaEndPicture` spam on hybrid GPUs; that does not restore Twitch playback.

Open codecs that *are* present (VP8/VP9/AV1) do not help when the site only offers H.264.

**Workaround:** open Twitch (and similar) in system Chrome/Firefox. Enabling codecs requires a **custom CEF/Chromium build** (`proprietary_codecs=true`, `ffmpeg_branding=Chrome`) plus awareness of patent/licensing constraints when redistributing. PyPI Qt WebEngine wheels have the same codec gap unless the distro build includes openh264.

## Security-related switches

Child-process command line currently includes aggressive flags used during embedding bring-up (`ignore-certificate-errors`, etc.). Treat as a personal tool; don’t assume hardened multi-user browser defaults.
