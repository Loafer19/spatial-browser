# Building and releasing

## CEF version pin

Keep these in sync:

- `crates/compositor/Cargo.toml` → `cef = "…"`
- `crates/cef-bridge/Cargo.toml` → `cef = "…"`
- `.github/workflows/ci.yml` / `release.yml` → `CEF_VERSION`
- README / `scripts/bundle.sh` comments for `cargo install cef --version …`

Pinned CEF: **154.5.0+154.0.34**.

Refresh local binaries after a bump:

```sh
cargo install cef --version "$CEF_VERSION" --locked --root ~/.local/share/cargo-cef-tools
cargo install export-cef-dir --version "$CEF_VERSION" --locked --root ~/.local/share/cargo-cef-tools
~/.local/share/cargo-cef-tools/bin/export-cef-dir --force ~/.local/share/cef
```

## Checks (same as CI)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings
cargo test --workspace -j 2
```

`-j 2` keeps rustc under ~8 GB on a 16 GB machine. Formatting-only commits go in `.git-blame-ignore-revs` (`git config blame.ignoreRevsFile .git-blame-ignore-revs` once per clone).

## Local bundle

```sh
./scripts/bundle.sh compositor   # → target/bundle/
./scripts/run.sh compositor      # run + crash auto-relaunch
./scripts/install.sh             # ~/.local/share/applications/…desktop
```

`bundle-cef-app` must be on `PATH` (from the cargo-cef-tools install). To try a CEF bump side by side, install tools and distribution to separate dirs and point the build at them: `CEF_PATH=~/.local/share/cef-<ver> CEF_TOOLS=~/.local/share/cargo-cef-tools-<ver> ./scripts/bundle.sh compositor`. `TMPDIR` may need a large disk — CEF’s dependency tree overflows small `/tmp` tmpfs.

## Release

Pushing a version tag triggers GitHub Actions:

```sh
git tag vX.Y.Z
git push origin vX.Y.Z
```

Workflow builds, bundles, and attaches `spatial-browser-linux-x86_64.tar.gz` to the GitHub Release. App version is the **git tag**, not `Cargo.toml` (`0.1.0` crates). Edit the release body afterward if auto-notes are too thin (match the style of prior tags).

## License

MIT — see [LICENSE](https://github.com/Loafer19/spatial-browser/blob/master/LICENSE).
