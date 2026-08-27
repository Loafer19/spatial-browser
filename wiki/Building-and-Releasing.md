# Building and releasing

## CEF version pin

Keep these in sync:

- `crates/compositor/Cargo.toml` → `cef = "…"`
- `crates/cef-bridge/Cargo.toml` → `cef = "…"`
- `.github/workflows/ci.yml` / `release.yml` → `CEF_VERSION`
- README / `scripts/bundle.sh` comments for `cargo install cef --version …`

Current (v0.7): **151.8.0+151.3.24**.

Refresh local binaries after a bump:

```sh
cargo install cef --version "$CEF_VERSION" --locked --root ~/.local/share/cargo-cef-tools
cargo install export-cef-dir --version "$CEF_VERSION" --locked --root ~/.local/share/cargo-cef-tools
~/.local/share/cargo-cef-tools/bin/export-cef-dir --force ~/.local/share/cef
```

## Local bundle

```sh
./scripts/bundle.sh compositor   # → target/bundle/
./scripts/run.sh compositor      # run + crash auto-relaunch
./scripts/install.sh             # ~/.local/share/applications/…desktop
```

`bundle-cef-app` must be on `PATH` (from the cargo-cef-tools install). `TMPDIR` may need a large disk — CEF’s dependency tree overflows small `/tmp` tmpfs.

## Release

Pushing a version tag triggers GitHub Actions:

```sh
git tag v0.7.0
git push origin v0.7.0
```

Workflow builds, bundles, and attaches `spatial-browser-linux-x86_64.tar.gz` to the GitHub Release. Edit the release body afterward if auto-notes are too thin (match the style of prior tags).

## License

MIT — see [LICENSE](https://github.com/Loafer19/spatial-browser/blob/master/LICENSE).
