use super::{color, Adapter, Patched};
use crate::detect::DetectResult;
use crate::env::Env;
use crate::palette::{ansi_name, Palette};
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use toml_edit::{value, DocumentMut, Item, Table, TableLike};

/// alacritty — `~/.config/alacritty/alacritty.toml` (TOML).
///
/// Uses `toml_edit` so untouched tables, comments and formatting survive
/// exactly. Legacy `alacritty.yml` is detected and reported as unsupported
/// rather than guessed at.
pub struct Alacritty;

impl Adapter for Alacritty {
    fn name(&self) -> &'static str {
        "alacritty"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        [
            "alacritty/alacritty.toml",
            "alacritty/alacritty.yml",
            "alacritty/alacritty.yaml",
        ]
        .iter()
        .filter_map(|p| env.app_config(p))
        .collect()
    }

    fn detect(&self, env: &Env) -> DetectResult {
        let Some(binary) = env.find_in_path("alacritty") else {
            return DetectResult::not_installed("alacritty");
        };
        let toml = env.app_config("alacritty/alacritty.toml");
        if let Some(p) = toml.filter(|p| p.exists()) {
            return DetectResult::compatible(binary, p);
        }
        for legacy in ["alacritty/alacritty.yml", "alacritty/alacritty.yaml"] {
            if let Some(p) = env.app_config(legacy).filter(|p| p.exists()) {
                return DetectResult::unsupported(
                    binary,
                    p,
                    "legacy YAML config found; migrate to alacritty.toml".to_string(),
                );
            }
        }
        DetectResult::config_missing(binary, "no alacritty.toml found".into())
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = env
            .app_config("alacritty/alacritty.toml")
            .filter(|p| p.exists())
            .ok_or_else(|| anyhow!("alacritty.toml not found"))?;
        let existing = std::fs::read_to_string(&path)?;
        let (content, keys) = render_alacritty(&existing, palette)?;
        Ok(Patched::single(path, content, keys))
    }
}

fn render_alacritty(existing: &str, palette: &Palette) -> Result<(String, Vec<String>)> {
    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| anyhow!("alacritty.toml is not valid TOML: {e}"))?;

    let mut touched = Vec::new();
    let mut set = |doc: &mut DocumentMut, path: &[&str], hex: &str| {
        let (table_path, leaf) = path.split_at(path.len() - 1);
        let table = ensure_table(&mut *doc, table_path);
        table.insert(leaf[0], value(hex));
        touched.push(path.join("."));
    };
    if let Some(fg) = color(palette, "foreground") {
        set(&mut doc, &["colors", "primary", "foreground"], &fg);
        set(&mut doc, &["colors", "cursor", "cursor"], &fg);
        set(&mut doc, &["colors", "selection", "text"], &fg);
    }
    if let Some(bg) = color(palette, "background") {
        set(&mut doc, &["colors", "primary", "background"], &bg);
        set(&mut doc, &["colors", "cursor", "text"], &bg);
    }
    // normal palette: indices 0..8, bright palette: indices 8..16
    for i in 0..16u8 {
        let Some(name) = ansi_name(i) else { continue };
        let Some(hex) = color(palette, name) else {
            continue;
        };
        if i < 8 {
            let key = ansi_name(i).unwrap();
            set(&mut doc, &["colors", "normal", key], &hex);
        } else {
            let key = ansi_name(i - 8).unwrap();
            set(&mut doc, &["colors", "bright", key], &hex);
        }
    }

    // app overrides address dotted TOML paths, e.g. "colors.selection.background"
    if let Some(overrides) = palette.overrides_for("alacritty") {
        for (dotted, raw) in overrides {
            if let Some(hex) = crate::palette::normalize_hex(raw) {
                let parts: Vec<&str> = dotted.split('.').collect();
                if parts.len() >= 2 {
                    set(&mut doc, &parts, &hex);
                }
            }
        }
    }

    Ok((doc.to_string(), touched))
}

/// Walk `path` under `root`, creating missing tables as standard (header)
/// tables. Existing tables — including inline tables — are entered and mutated
/// in place so their other keys and formatting are preserved.
fn ensure_table<'a>(root: &'a mut Table, path: &[&str]) -> &'a mut dyn TableLike {
    let mut cur: &mut dyn TableLike = root;
    for key in path {
        let needs_create = cur.get(key).map(|i| !i.is_table_like()).unwrap_or(true);
        if needs_create {
            cur.insert(key, Item::Table(Table::new()));
        }
        cur = cur
            .get_mut(key)
            .and_then(Item::as_table_like_mut)
            .expect("ensured table-like");
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn patches_annotated_toml_preserving_comments() {
        let existing = "# my alacritty\n[window]\npadding = { x = 4, y = 4 } # keep\n\n[colors.primary]\nbackground = \"#000000\"\n";
        let (out, keys) = render_alacritty(existing, &palette()).unwrap();
        assert!(out.contains("# my alacritty"));
        assert!(out.contains("padding = { x = 4, y = 4 } # keep"));
        assert!(out.contains("background = \"#111111\""));
        assert!(out.contains("foreground = \"#eeeeee\""));
        assert!(out.contains("[colors.normal]"));
        assert!(out.contains("black = \"#000000\""));
        assert!(out.contains("[colors.bright]"));
        assert!(keys.contains(&"colors.normal.black".to_string()));
    }

    #[test]
    fn invalid_toml_errors() {
        assert!(render_alacritty("this is : not toml", &palette()).is_err());
    }

    #[test]
    fn preserves_siblings_in_inline_colors_table() {
        let existing = "[colors]\nprimary = { foreground = \"#000000\", background = \"#111111\", dim_foreground = \"#cccccc\" }\n";
        let (out, _) = render_alacritty(existing, &palette()).unwrap();
        // The unmanaged sibling key must survive.
        assert!(
            out.contains("dim_foreground"),
            "lost dim_foreground:\n{out}"
        );
        assert!(out.contains("foreground = \"#eeeeee\""));
        assert!(out.contains("background = \"#111111\""));
    }
}
