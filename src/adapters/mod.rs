use crate::detect::{detect_with, DetectResult};
use crate::env::Env;
use crate::palette::{normalize_hex, Palette};
use anyhow::Result;
use std::path::PathBuf;

pub mod alacritty;
pub mod btop;
pub mod foot;
pub mod ghostty;
pub mod kitty;
pub mod micro;
pub mod superfile;
pub mod tmux;

/// One file the adapter wants written.
#[derive(Debug, Clone)]
pub struct FilePatch {
    pub path: PathBuf,
    pub content: String,
}

/// The complete result of an adapter patching a palette.
#[derive(Debug, Clone)]
pub struct Patched {
    pub files: Vec<FilePatch>,
    /// App-specific keys the adapter mapped/touched, for `status` auditing.
    pub keys_touched: Vec<String>,
}

impl Patched {
    pub fn single(path: PathBuf, content: String, keys: Vec<String>) -> Patched {
        Patched {
            files: vec![FilePatch { path, content }],
            keys_touched: keys,
        }
    }
}

/// Built-in adapter for one application.
pub trait Adapter {
    /// Registry name (also the `app_overrides` key).
    fn name(&self) -> &'static str;
    /// Executable name to look for on `$PATH`. Defaults to `name`.
    fn binary(&self) -> &'static str {
        self.name()
    }
    /// Candidate config paths, in priority order.
    fn config_candidates(&self, env: &Env) -> Vec<PathBuf>;
    /// Parse a palette into the file(s) this app consumes.
    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched>;

    /// Default detection: binary on `$PATH` AND at least one candidate exists.
    fn detect(&self, env: &Env) -> DetectResult {
        detect_with(
            env,
            self.name(),
            self.binary(),
            &self.config_candidates(env),
        )
    }

    /// Path reported by `list`/`status` (the first existing candidate, if any).
    fn resolved_config(&self, env: &Env) -> Option<PathBuf> {
        self.config_candidates(env).into_iter().find(|p| p.exists())
    }
}

/// All built-in adapters, in the phase order from the plan.
pub fn registry() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(kitty::Kitty),
        Box::new(foot::Foot),
        Box::new(alacritty::Alacritty),
        Box::new(micro::Micro),
        Box::new(ghostty::Ghostty),
        Box::new(superfile::Superfile),
        Box::new(btop::Btop),
        Box::new(tmux::Tmux),
    ]
}

/// Look up a canonical color, normalised to `#rrggbb`.
pub fn color(palette: &Palette, key: &str) -> Option<String> {
    palette.color(key).and_then(normalize_hex)
}

/// Merge an app's overrides into a key/value map (overrides win).
pub fn apply_overrides(map: &mut indexmap::IndexMap<String, String>, palette: &Palette, app: &str) {
    if let Some(overrides) = palette.overrides_for(app) {
        for (key, value) in overrides {
            if let Some(hex) = normalize_hex(value) {
                map.insert(key.clone(), hex);
            }
        }
    }
}

/// Convenience: a fresh ordered map with an optional explicit capacity.
pub fn map() -> indexmap::IndexMap<String, String> {
    indexmap::IndexMap::new()
}

#[cfg(test)]
pub(crate) mod testpalette {
    use crate::palette::Palette;

    /// A deterministic 16-color + fg/bg palette used across adapter tests.
    pub fn palette() -> Palette {
        Palette::from_yaml(
            r##"
name: test
palette:
  black:        "#000000"
  red:          "#ff0000"
  green:        "#00ff00"
  yellow:       "#ffff00"
  blue:         "#0000ff"
  magenta:      "#ff00ff"
  cyan:         "#00ffff"
  white:        "#ffffff"
  brightBlack:  "#555555"
  brightRed:    "#ff5555"
  brightGreen:  "#55ff55"
  brightYellow: "#ffff55"
  brightBlue:   "#5555ff"
  brightMagenta: "#ff55ff"
  brightCyan:   "#55ffff"
  brightWhite:  "#ffffff"
  foreground:   "#eeeeee"
  background:   "#111111"
"##,
        )
        .expect("valid test palette")
    }
}
