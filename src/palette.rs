use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The 16 ANSI colors plus `foreground`/`background`, in canonical order.
///
/// Canonical names are deliberately broader than any single app's key naming
/// so one palette file can drive every adapter. `bright*` uses camelCase in the
/// file; lookups are normalised, so `brightBlack`, `bright_black` and
/// `brightblack` all resolve to the same key.
pub const CANONICAL_KEYS: [&str; 18] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
    "foreground",
    "background",
];

/// Sources of a single resolved color value.
///
/// A palette is the user's canonical mapping; app overrides let a user express
/// app-specific keys (for example tmux `base00`) without polluting the core.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Palette {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Canonical color name -> hex string.
    #[serde(default)]
    pub palette: IndexMap<String, String>,
    /// app name -> (app-specific output key -> hex). Applied after the
    /// adapter's own mapping, so overrides always win.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub app_overrides: IndexMap<String, IndexMap<String, String>>,
}

impl Palette {
    pub fn load(path: &Path) -> Result<Palette> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading palette file {}", path.display()))?;
        Palette::from_yaml(&text).with_context(|| format!("parsing palette {}", path.display()))
    }

    pub fn from_yaml(text: &str) -> Result<Palette> {
        let palette: Palette = serde_yaml::from_str(text)?;
        palette.validate()?;
        Ok(palette)
    }

    /// Ensure every provided color is a well-formed hex value and no unknown
    /// canonical keys are present (unknown keys almost always indicate a typo,
    /// which would otherwise silently do nothing).
    pub fn validate(&self) -> Result<()> {
        for (key, value) in &self.palette {
            let norm = normalize_key(key);
            if !CANONICAL_KEYS.iter().any(|c| normalize_key(c) == norm) {
                bail!(
                    "unknown palette key `{key}` (expected one of: {})",
                    CANONICAL_KEYS.join(", ")
                );
            }
            normalize_hex(value)
                .ok_or_else(|| anyhow!("palette key `{key}` has invalid hex color `{value}`"))?;
        }
        for (app, overrides) in &self.app_overrides {
            for (key, value) in overrides {
                if !is_safe_override_key(key) {
                    bail!(
                        "app_overrides.{app}: unsafe override key `{key}` \
                         (allowed characters: letters, digits, '.', '_', '-', '@')"
                    );
                }
                normalize_hex(value).ok_or_else(|| {
                    anyhow!("app_overrides.{app}.{key} has invalid hex color `{value}`")
                })?;
            }
        }
        Ok(())
    }

    /// Canonical lookup with tolerant key matching.
    pub fn color(&self, key: &str) -> Option<&str> {
        let want = normalize_key(key);
        self.palette
            .iter()
            .find(|(k, _)| normalize_key(k) == want)
            .map(|(_, v)| v.as_str())
    }

    /// App-specific overrides for one adapter.
    pub fn overrides_for(&self, app: &str) -> Option<&IndexMap<String, String>> {
        self.app_overrides.get(app)
    }
}

/// Normalise a key for comparison: lowercase, drop `_`/`-`.
pub fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Whether an `app_overrides` key is safe to interpolate into a generated
/// config directive. Adapter output embeds override keys verbatim, so anything
/// that could break out of a line or quote (whitespace, newlines, quotes) or
/// target an unexpected structural character is rejected.
pub fn is_safe_override_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
}

/// Validate and normalise a hex color to lowercase `#rrggbb` (or `#rgb` stays
/// as-is is not needed; we always expand to 6 digits).
pub fn normalize_hex(value: &str) -> Option<String> {
    let v = value.trim();
    let hex = v.strip_prefix('#').unwrap_or(v);
    let expanded = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    if expanded.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{}", expanded.to_lowercase()))
    } else {
        None
    }
}

/// The canonical name for a given ANSI index, if any.
pub fn ansi_name(index: u8) -> Option<&'static str> {
    CANONICAL_KEYS.get(index as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
name: dracula
palette:
  black: "#282a36"
  red: "#ff5555"
  green: "#50fa7b"
  yellow: "#f1fa8c"
  blue: "#bd93f9"
  magenta: "#ff79c6"
  cyan: "#8be9fd"
  white: "#f8f8f2"
  brightBlack: "#44475a"
  brightRed: "#ff6e67"
  brightGreen: "#69ff94"
  brightYellow: "#ffff86"
  brightBlue: "#d6acff"
  brightMagenta: "#ff92df"
  brightCyan: "#a4ffff"
  brightWhite: "#ffffff"
  foreground: "#f8f8f2"
  background: "#282a36"
"##;

    #[test]
    fn loads_and_normalises() {
        let p = Palette::from_yaml(SAMPLE).unwrap();
        assert_eq!(p.name.as_deref(), Some("dracula"));
        assert_eq!(p.color("red"), Some("#ff5555"));
        assert_eq!(p.color("bright_red"), Some("#ff6e67"));
        assert_eq!(ansi_name(15), Some("brightWhite"));
    }

    #[test]
    fn rejects_bad_hex() {
        let bad = "palette:\n  red: \"not-a-color\"\n";
        assert!(Palette::from_yaml(bad).is_err());
    }

    #[test]
    fn rejects_unknown_key() {
        let bad = "palette:\n  reddish: \"#ff0000\"\n";
        assert!(Palette::from_yaml(bad).is_err());
    }

    #[test]
    fn three_digit_hex_expands() {
        assert_eq!(normalize_hex("#abc").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("ABC").as_deref(), Some("#aabbcc"));
    }

    #[test]
    fn overrides_load() {
        let with_over = format!("{SAMPLE}\napp_overrides:\n  tmux:\n    base00: \"#282a36\"\n");
        let p = Palette::from_yaml(&with_over).unwrap();
        assert_eq!(
            p.overrides_for("tmux").unwrap().get("base00").unwrap(),
            "#282a36"
        );
    }

    #[test]
    fn rejects_unsafe_override_key() {
        // A key that could break out of a generated config line must be rejected.
        let bad = format!("{SAMPLE}\napp_overrides:\n  tmux:\n    \"x\\ny\": \"#282a36\"\n");
        assert!(Palette::from_yaml(&bad).is_err());
        let bad2 = format!("{SAMPLE}\napp_overrides:\n  kitty:\n    \"a b\": \"#282a36\"\n");
        assert!(Palette::from_yaml(&bad2).is_err());
    }

    #[test]
    fn safe_override_keys() {
        assert!(is_safe_override_key("base00"));
        assert!(is_safe_override_key("colors.selection.background"));
        assert!(is_safe_override_key("@my_var"));
        assert!(!is_safe_override_key(""));
        assert!(!is_safe_override_key("a\nb"));
        assert!(!is_safe_override_key("a\"b"));
        assert!(!is_safe_override_key("a;b"));
    }
}
