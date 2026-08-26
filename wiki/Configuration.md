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
├── workspaces.json
├── settings.json
├── userscripts/           # your .js files
├── userscripts_state.json # disabled script basenames
└── instance.sock          # single-instance lock
```

## Settings UI (`Ctrl+,`)

Persisted in `settings.json`:

- Ad-block on/off
- Clean URLs on/off
- Default search engine
- UI theme
- Reader theme
- Extra blocked hosts
- Target frame rate (60/90/120)

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

Build/runtime CEF distribution path: `CEF_PATH` (default `~/.local/share/cef` via `.cargo/config.toml`). Must match the `cef` crate version — see [[Building-and-Releasing]].
