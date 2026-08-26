# Password manager

Local encrypted vault + form autofill. Not a Chrome/Bitwarden sync client — see [[Limitations]].

## Quick use

1. **Ctrl+Shift+P** — create vault (first time) or unlock with master password
2. Browse a site, focus a login field — autofill runs if the vault is unlocked
3. Multiple logins for one site → picker at the bottom-right
4. After you submit a new/changed login → **Save password?** banner
5. Generator + manual add + never-save list live on the passwords page

Unlock lasts for the **browser process** (until exit).

## Storage

- File: `~/.config/spatial-browser/vault.enc`
- Argon2id + AES-256-GCM
- Entries: origin, username, password, optional email / address fields
- `never_save`: origins that suppress save prompts

## Autofill

Injected bridge (with clipboard bridge) on normal pages:

- Detects password forms
- Queries Rust via `password://…` (fake navigations, canceled)
- Fills username/password and email when present

Built-in `spatial-ui` pages are **not** autofilled.

## Security notes

- Personal local tool; master password never leaves the machine
- While unlocked, secrets sit in process memory
- `password://` URLs may briefly carry secrets on confirm — don’t leave remote debugging on

## Deferred

- [ ] CSV import (Chrome / Bitwarden export)
- [ ] Idle auto-lock / lock hotkey / lock-on-sleep
- [ ] TOTP / 2FA codes
- [ ] Passkeys / WebAuthn
