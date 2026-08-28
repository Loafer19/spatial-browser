# Password manager

Local encrypted vault + form autofill. Not a Chrome/Bitwarden sync client — see [[Limitations]].

## Quick use

1. **Ctrl+Shift+P** — create vault (first time) or unlock with master password
2. Browse a site, focus a login field — **Use saved login?** chip (bottom-left) if the vault is unlocked (click to fill; never silent)
3. After you submit a **new or changed** login → **Save password?** chip (same corner). Identical username+password is not offered again
4. Passwords page tabs: **Saved** · **Add / Import** · **Generator** · **Never save**

Unlock lasts for the **browser process** (until exit).

## Storage

- File: `~/.config/spatial-browser/vault.enc`
- Argon2id + AES-256-GCM
- Entries: origin, username, password, optional email / address fields
- `never_save`: origins that suppress save prompts

## Import CSV

On the passwords page (**Ctrl+Shift+P** → unlocked):

1. Export from Chrome (*Passwords → ⋮ → Export*) or Bitwarden (*Tools → Export vault → .csv*)
2. Click **Choose CSV file…** — system file dialog (XDG portal on Linux)
3. Matching is by `(origin, username)` — same login updates the password; duplicates with identical data are skipped

Supported headers:

- Chrome: `url`, `username`, `password`
- Bitwarden: `login_uri`, `login_username`, `login_password` (non-`login` rows skipped)

## Autofill / suggest fill

Injected bridge (with clipboard bridge) on normal pages:

- Detects password / username fields on focus
- Queries Rust via `password://…` (fake navigations, canceled)
- Shows a bottom-right **Use saved login?** chip for any matches — fill only after you click
- Context menu → Fill password uses the same suggestion path

Built-in `spatial-ui` pages are **not** autofilled.

## Security notes

- Personal local tool; master password never leaves the machine
- While unlocked, secrets sit in process memory
- `password://` URLs may briefly carry secrets on confirm — don’t leave remote debugging on
- CSV import opens a system file dialog; delete the export file after importing

## Deferred

- [x] CSV import (Chrome / Bitwarden export)
- [x] Native file picker for import
- [ ] Idle auto-lock / lock hotkey / lock-on-sleep
- [ ] TOTP / 2FA codes
- [ ] Passkeys / WebAuthn
