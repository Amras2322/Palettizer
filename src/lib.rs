//! Palettizer — sync one source-of-truth YAML palette to compatible TUI apps.
//!
//! The crate is split into small layers so the CLI stays thin and the logic is
//! testable against a synthetic environment:
//!
//! * [`env`] — portable `$HOME`/`$XDG_CONFIG_HOME`/`$PATH` resolution.
//! * [`palette`] — the palette model plus YAML load/validation.
//! * [`detect`] — compatibility probing (binary on `$PATH` + config exists).
//! * [`adapters`] — one built-in adapter per application.
//! * [`backup`] — timestamped backups and rollback manifests.
//! * [`engine`] — orchestrates detect → backup → patch → write → report.
//! * [`textedit`] — line-based config patching helpers.

pub mod adapters;
pub mod backup;
pub mod detect;
pub mod engine;
pub mod env;
pub mod palette;
pub mod textedit;
