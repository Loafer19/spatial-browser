# Password manager

Local encrypted vault + form autofill. Not a Chrome/Bitwarden sync client — see [[Limitations]].

## Quick use

1. **Ctrl+Shift+P** — create vault (first time) or unlock with master password
2. Browse a site, focus a login field — **Use saved login?** chip (bottom-left) if the vault is unlocked (click to fill; never silent)
3. After you submit a **new or changed** login → **Save password?** chip (same corner). Identical username+password is not offered again
4. Passwords page tabs: **Saved** · **Add / Import** · **Generator** · **Never save**

Locking: **Ctrl+Shift+L** or the **Lock** button on the passwords page. The vault also locks itself after **15 minutes** without use (list page actions, Ctrl+Shift+P, fill/save clicks; focus queries don't count). Locking drops the key and decrypted entries from memory and turns any open passwords page back into the unlock form.

## Storage

- File: `~/.config/spatial-browser/vault.enc`, mode `0600`
- Argon2id → AES-256-GCM. New vaults: m=64 MiB, t=3, p=1 (~0.1 s on a desktop)
- Entries: origin, username, password, optional email / address fields
- `never_save`: origins that suppress save prompts
- Writes are atomic: temp file (`0600`) → fsync → rename → fsync dir. A crash leaves the old or the new vault, never half of one

File format (little-endian):

| Version | Layout | KDF |
|---|---|---|
| v1 (read only) | `SBVT` · `01` · salt[16] · nonce[12] · ciphertext+tag | implicit argon2 0.5 default (m=19 MiB, t=2, p=1) |
| v2 (written) | `SBVT` · `02` · m_cost u32 (KiB) · t_cost u32 · p_cost u32 · salt[16] · nonce[12] · ciphertext+tag | params from header; header is the GCM AAD |

Unlocking a v1 vault, or a v2 vault with weaker params than current, re-encrypts it in place with fresh salt and nonce. If that write fails, the old file stays and the session keeps working. Header params outside sane bounds (m ≤ 1 GiB, t ≤ 32, p ≤ 16) are rejected before Argon2 runs.

Memory: key, entries, plaintext JSON buffers, the master password from the unlock form and the generator output are zeroized on drop/lock. Not reachable: serde_json parse scratch, strings CEF decoded from `password://` URLs, and old copies left by String reallocation.

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

**Where it works:** HTTPS pages, plus plain HTTP only on this machine and the local network — `localhost` / `*.localhost`, `*.local`, loopback, private (10/8, 172.16/12, 192.168/16) and link-local addresses, IPv6 ULA/link-local. Public `http://` sites get no suggestions and no save prompt: anyone on the path can read or rewrite those pages. Entries match by host (`www.` ignored), and a page can only query, fill or save entries for its own main-frame origin.

## Security notes

- Personal local tool; master password never leaves the machine
- While unlocked, secrets sit in process memory (scrubbed on lock; see Storage)
- `password://` URLs may briefly carry secrets on confirm — don’t leave remote debugging on
- CSV import opens a system file dialog; delete the export file after importing

## Deferred

- [x] CSV import (Chrome / Bitwarden export)
- [x] Native file picker for import
- [x] Lock hotkey (Ctrl+Shift+L) + Lock button
- [x] Idle auto-lock (15 min, const `vault::AUTO_LOCK_AFTER`)
- [x] Versioned KDF params in the file header, auto-migration
- [ ] Auto-lock timeout as a Settings field
- [ ] Lock on suspend / screen lock (logind `PrepareForSleep`)
- [ ] TOTP / 2FA codes
- [ ] Passkeys / WebAuthn
