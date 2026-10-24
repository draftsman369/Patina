# Contributing to Patina

Thanks for your interest! Patina is GPL-3.0-or-later; by contributing you agree
that your work is released under that license.

## Before you start

- Check [ROADMAP.md](ROADMAP.md) to see which milestone a feature belongs to.
- For large features (compositor, brush engine, file format…), open a design
  discussion or an RFC in `docs/rfcs/` first.
- Patina must never include Adobe code, assets, icons, branding, shelf content
  or proprietary formats. Only CC0/CC-BY assets may be added, each folder with
  a license manifest.

## Development

```sh
cargo run -p patina_app              # run the editor (dependencies are optimized even in debug)
cargo test --workspace               # unit + GPU snapshot tests
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

### GPU snapshot tests

`crates/patina_render/tests/snapshot.rs` renders fixed scenes and compares
them with `tests/snapshots/*.png`, tolerating small rasterizer differences.

| Variable | Effect |
|---|---|
| `PATINA_UPDATE_SNAPSHOTS=1` | Rewrite the reference images (review the diff before committing). |
| `PATINA_SOFTWARE_GPU=1` | Use a software adapter (lavapipe, WARP), as CI does. |
| `PATINA_REQUIRE_GPU=1` | Fail instead of skipping when no GPU is available. |

Failed comparisons write the actual image and a red diff mask to
`target/snapshot-failures/`.

## Code style

- `rustfmt` defaults; clippy must pass with `-D warnings`.
- Keep `patina_core` free of GPU and UI code.
- Comments explain *why*; keep them short.
- Add tests with every change: unit tests for logic, a snapshot for rendering changes.

## Commits and pull requests

- Small, focused commits with an imperative subject line ("Add torus primitive").
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).
- CI must be green on Windows, Linux and macOS.
