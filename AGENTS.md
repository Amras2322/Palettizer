# AGENTS.md

Guidance for agents working in this repository.

## What this is

`palettizer` — a Rust CLI that patches TUI app config files from a single YAML
palette. See `README.md` for user-facing docs.

## Commands

```sh
cargo build                     # debug build
cargo build --release           # optimized binary
cargo test                      # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt                       # format
cargo fmt --check               # verify formatting
```

Always run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test` before considering work done.

## Layout

```
src/lib.rs        crate root, re-exports the layers
src/main.rs       CLI (clap) — thin; formats reports, decides exit codes
src/env.rs        Env: portable $HOME/$XDG_CONFIG_HOME/$PATH resolution
src/palette.rs    Palette model, YAML load, validation, key normalisation
src/detect.rs     DetectStatus/DetectResult + detect_with helper
src/backup.rs     BackupStore, manifests, rollback, UTC timestamps
src/engine.rs     run_apply / run_status / run_rollback orchestration
src/textedit.rs   line/INI patching that preserves untouched lines
src/adapters/     one module per app + Adapter trait + registry()
tests/e2e.rs      end-to-end tests against a synthetic home
```

## Conventions

- **Portability:** never call `std::env` or Linux-specific syscalls in core
  logic. Everything goes through `Env`. New platforms add a branch in
  `Env::config_home` only.
- **Non-destructive patching:** only touch known palette keys. All other lines
  must remain byte-identical. Adapters return keys touched for `status`.
- **Non-fatal errors:** a failing adapter must be recorded on its `AppReport`
  and must not abort the run.
- **Backups first:** any file about to change is backed up before writing;
  generated files are recorded with `backup: None` so rollback deletes them.
- **Adapter interface:** to implement the `Adapter` trait you need `name`,
  `config_candidates`, and `patch`; override `detect` only for multi-format apps.
  Register new adapters in `adapters::registry()`.

## Adding an adapter

1. Add `src/adapters/<app>.rs` with a unit test using
   `adapters::testpalette::palette()`.
2. Register it in `src/adapters/mod.rs`.
3. Add an entry to the README table and a golden/e2e test if the format is
   non-trivial.
