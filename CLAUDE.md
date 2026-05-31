# CLAUDE.md

Guidance for working on **rproc** — a resource & process monitor for Linux,
built with `eframe`/`egui`. This file captures the project's conventions so
changes stay consistent and pass CI on the first try.

## Project shape

- Rust **edition 2024**, binary crate `rproc`.
- GUI on `eframe`/`egui` (immediate-mode); system data via `sysinfo`, GPU via
  `nvml-wrapper`.
- Layout:
  - `src/main.rs` — entry point.
  - `src/app.rs` — top-level `eframe::App` state and frame loop.
  - `src/monitor/` — data collection (system, processes, services, startup,
    gpu, sampler). No UI here.
  - `src/ui/` — egui rendering per tab (performance, processes, services,
    startup, settings) plus `sidebar`, `widgets`, `icons`. No sampling logic
    here.
  - `src/daemon/` — background sampling daemon (`pidfile`, `storage`).
  - `src/settings.rs`, `src/theme.rs` — config and styling.

Keep the **monitor (data) / ui (render)** separation intact: collection logic
belongs under `src/monitor`, drawing belongs under `src/ui`. New optional or
self-contained features go in their own module with minimal hooks elsewhere.

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
  with explicit feature lists where the project already does (see `eframe`,
  `egui_extras`, `image`).
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
  enumeration) belongs in the monitor/daemon layer, not inline in a frame
  callback. The frame loop must stay responsive.
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
