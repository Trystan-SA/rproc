# CLAUDE.md

Guidance for working on **rproc** — a resource & process monitor for Linux,
built with `Slint` (software renderer). This file captures the project's
conventions so changes stay consistent and pass CI on the first try.

## Project shape

- Rust **edition 2024**, binary crate `rproc`.
- GUI on `Slint`, rendered by the **software backend** (no GPU context, lowest
  resident footprint); system data via `sysinfo`, GPU via `nvml-wrapper`.
- The `.slint` views live at the crate root under `ui/` and are compiled by
  `build.rs` (`slint::include_modules!()` in `main.rs` generates the Rust
  bindings). `ui/` holds `app.slint` (the `MainWindow`), one file per tab,
  plus `sidebar`, `widgets`, `structs`, `theme`.
- Layout:
  - `src/main.rs` — entry point + `slint::include_modules!()`.
  - `src/app.rs` — owns the `MainWindow`, the `Sampler`, per-tab state and the
    `slint::Timer` that polls the snapshot and drives all callbacks.
  - `src/monitor/` — data collection (system, processes, services, startup,
    gpu, sampler). No UI here.
  - `src/ui/` — the **glue**: each tab maps a `Snapshot` into the Slint models
    (`performance`, `processes`, `services`, `startup`, `settings`), plus
    `icons`, `graph`, `model`, and the pure formatting/sorting/filter helpers.
    No drawing here (that's the `.slint` files).
  - `src/daemon/` — background sampling daemon (`pidfile`, `storage`).
  - `src/settings.rs`, `src/theme.rs` — config and a few `slint::Color`s.

Keep the **monitor (data) / ui (glue) / .slint (render)** separation intact:
collection belongs under `src/monitor`, model-building under `src/ui`, drawing
in `ui/*.slint`. The UI is retained-mode: update Slint model properties from
the glue, don't try to "redraw". New optional or self-contained features go in
their own module with minimal hooks elsewhere.

### Slint / software-renderer gotchas

- The software renderer can't take a dynamic `Path.commands` string, and `for`
  is not allowed inside a `Path` — use a fixed set of child line elements
  (graphs are 60-point polylines fed exactly 60 values; see `src/ui/graph.rs`).
- Don't replace a list's `ModelRc` every tick (it recreates delegates and drops
  in-flight clicks); reconcile a persistent `VecModel` in place via
  `ui::model::sync`.
- `Rectangle`'s `color` is reserved; name custom color props something else.
- std-widgets follow the system light scheme — `MainWindow` forces
  `Palette.color-scheme = ColorScheme.dark` so their scrollbars/sliders match.

## The hygiene loop — run before every commit

CI runs `fmt`, `clippy`, `test`, and `build`, all with `RUSTFLAGS="-D warnings"`.
Mirror that locally so nothing fails after push:

```sh
cargo fmt --all                              # format (CI checks with --check)
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
cargo build --release --locked
```

- **Warnings are errors.** CI sets `-D warnings`; do not merge code that warns.
  Don't blanket-`#[allow(...)]` to silence clippy — fix the cause, or add a
  narrowly scoped allow with a comment explaining why.
- **`--locked` is mandatory.** `Cargo.lock` is committed and CI builds with
  `--locked`. Never let a build silently update the lockfile; if you change
  deps, commit the updated `Cargo.lock` in the same change.
- Run `cargo fmt` before committing — the `Format` job fails on any diff.

## Dependencies

- Add deps deliberately; this is a lean monitor. Prefer the standard library
  and existing crates over pulling in new ones.
- Pin sensible versions in `Cargo.toml` and keep `default-features = false`
  with explicit feature lists where the project already does (see `slint`,
  `image`, `resvg`). `slint` is pinned to an exact version and enables only
  `backend-winit`, `renderer-software` and `software-renderer-path`; `resvg`
  drops its `text` feature (icons are shapes, and `text` would pull `fontdb` +
  system fontconfig).
- **System build deps:** Slint's winit/font stack links `fontconfig` on Linux,
  so building needs `libfontconfig1-dev` (plus `libxkbcommon-dev`, `libx11-dev`,
  `libxcb1-dev`, `libwayland-dev`). The CI/release workflows install these; keep
  them in sync if the dep set changes. No OpenGL dev package is needed (software
  renderer).
- After any dependency change: `cargo build --locked` and commit the resulting
  `Cargo.lock`.
- The release profile is tuned for a small binary (`lto = "thin"`,
  `codegen-units = 1`, `strip`, `panic = "abort"`). Don't undo these casually —
  note that `panic = "abort"` means no unwinding, so don't write code that
  relies on catching panics.

## Rust conventions

- **Error handling:** use `anyhow::Result` with `?` for fallible paths
  (`anyhow` is the project's error crate). Add context with `.context(...)`
  rather than bare `?` when the failure site would otherwise be ambiguous.
- **No panics on the hot path.** This is a long-running UI sampling at a fixed
  cadence — avoid `unwrap()`/`expect()` on values that depend on the live
  system (process lists, GPU handles, file reads). Reserve `expect()` for
  genuine invariants and give it a message that states the invariant.
- **`unsafe`:** the crate uses `libc` for some syscalls. Any `unsafe` block
  must carry a `// SAFETY:` comment justifying it. Keep `unsafe` minimal and
  wrapped in a safe API.
- **Don't block the UI thread.** Sampling that can stall (disk, NVML, process
  enumeration) runs on the sampler thread; the UI only reads the published
  `Snapshot`. Keep blocking work out of the timer tick and Slint callbacks so
  the event loop stays responsive.
- Follow existing idioms: borrow over clone where cheap, iterators over manual
  loops, `match` over nested `if let` when it reads clearer. Match the
  surrounding code's naming and comment density.

## Comments & docs

- **Self-explanatory code first.** Clear names and small functions beat
  comments. If a comment is needed to explain *what* code does, rename or
  refactor instead.
- Keep comments sleek and rare. Comment only the *why* behind a non-obvious
  decision (see the `panic = "abort"` note in `Cargo.toml`) — never the *what*.
  Avoid comment blocks and section banners.
- A short `///` on a public item is fine when the name alone isn't enough;
  don't document the obvious.

## Tests

- **Keep tests in separate files, not inline with the working code.** Don't
  write `#[cfg(test)] mod tests { ... }` at the bottom of a source file.
  Instead declare the test module and point it at its own file:

  ```rust
  // in src/monitor/processes.rs — one line, no test bodies here
  #[cfg(test)]
  mod tests;
  ```

  ```rust
  // in src/monitor/processes/tests.rs — the tests live here
  use super::*;

  #[test]
  fn sorts_by_cpu_descending() { /* ... */ }
  ```

  This keeps the working module readable while the test file still has access
  to the module's private items via `use super::*;`.
- Cover pure data transforms in `monitor` (parsing, aggregation, sorting)
  especially. UI rendering is hard to unit-test — keep logic out of draw code
  so it *can* be tested from a sibling test file.
- `cargo test --all-features --locked` must pass before pushing.

## Packaging & release (don't touch unless asked)

- Packaging lives in `packaging/` and the `Makefile` (`make deb`, `make rpm`,
  `make flatpak`). The version is read from `Cargo.toml`.
- Releases go through `make release` (interactive: bump, tag `vX.Y.Z`, push) —
  CI's release workflow then publishes artifacts. Don't hand-edit version
  numbers or tags outside that flow.

## Pull requests

- Keep PRs focused and the diff minimal — match the prefix style of recent
  commits (`fix(ui):`, `feat(ui):`, `chore:`).
- Before opening a PR, confirm the full hygiene loop above passes locally.
