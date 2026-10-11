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

Tabs **General** | **Blocking** | **Appearance** | **Performance**, plus a search box (`/`) that finds a setting in any tab. Persisted in `settings.json` (older files load unchanged; unknown values fall back to defaults):

- **General:** clean URLs, default search engine (presets or a custom `http(s)` URL the query is appended to), links to Scripts & styles and Keyboard shortcuts
- **Blocking:** content filtering master, filter lists, extra blocked hosts, network / cosmetic / scriptlet layers
- **Appearance:** UI theme, reader theme
- **Performance:** target frame rate (60/90/120, applies immediately); **GPU** (`low` integrated / `high` discrete) and **page painting** (`auto` / `gpu` shared textures / `cpu` copy) — these two **apply after restart**; the page shows a banner while a restart is pending and marks what is running now
- Last opened settings tab

`auto` painting uses shared textures on the integrated GPU and CPU copies on the discrete GPU (hybrid laptops can't share textures across GPUs).

## Environment overrides

| Variable | Values | Effect |
|----------|--------|--------|
| `SPATIAL_BROWSER_OSR` | `cpu` / `gpu` / `auto` | Force CPU `on_paint` or shared-texture OSR; wins over Settings → Performance |
| `SPATIAL_BROWSER_GPU` | `low` / `high` (also `integrated` / `discrete`) | wgpu adapter preference; wins over Settings → Performance |

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
