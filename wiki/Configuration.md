# Configuration

Everything user-facing lives under:

```
~/.config/spatial-browser/
├── cef_data/              # CEF profile (cookies, local storage, …)
├── session.json           # open pages + viewport (debounced save)
├── bookmarks.json
├── history.json
├── downloads.json
├── typed_history.json
├── workspaces.json        # live slots (v2); autosaved on switch
├── filters/               # EasyList, EasyPrivacy, scriptlets.js (seeded from bundle)
├── settings.json
├── userscripts/           # your .js files
├── userscripts_state.json # disabled script basenames
├── userstyles/            # your .css files
├── userstyles_state.json  # disabled style basenames
├── vault.enc              # encrypted password vault (after first create)
└── instance.sock          # single-instance lock
```

## Settings UI (`Ctrl+,`)

Tabbed page: **General** | **Blocking** | **Appearance**. Persisted in `settings.json`:

- Content filtering master + filter-list toggles + network/cosmetic/scriptlets layers
- Clean URLs on/off
- Default search engine
- UI theme / reader theme
- Extra blocked hosts
- Target frame rate (60/90/120)
- Last opened settings tab

## Environment overrides

| Variable | Values | Effect |
|----------|--------|--------|
| `SPATIAL_BROWSER_OSR` | `cpu` / `gpu` | Force CPU `on_paint` or shared-texture OSR |
| `SPATIAL_BROWSER_GPU` | `low` / `high` (also `integrated` / `discrete`) | wgpu adapter preference |

Defaults (v0.5+): shared-texture OSR on, wgpu **LowPower** (iGPU) so hybrid laptops keep CEF and the compositor on the same GPU for DMA-BUF import.

If pages render black after enabling GPU OSR:

```sh
SPATIAL_BROWSER_OSR=cpu ./scripts/run.sh compositor
```

For discrete GPU compositing without shared textures:

```sh
SPATIAL_BROWSER_GPU=high SPATIAL_BROWSER_OSR=cpu ./scripts/run.sh compositor
```

## CEF binaries

Build/runtime CEF distribution path: `CEF_PATH` (typically `~/.local/share/cef`). Set via local `.cargo/config.toml` (from `config.toml.example`) or the environment. Must match the `cef` crate version — see [[Building-and-Releasing]].
