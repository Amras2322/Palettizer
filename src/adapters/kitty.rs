use super::{apply_overrides, color, map, Adapter, Patched};
use crate::env::Env;
use crate::palette::{ansi_name, Palette};
use crate::textedit::{leading_key, patch_assignments, Insert};
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// kitty — `~/.config/kitty/kitty.conf`, flat `key value` lines.
pub struct Kitty;

impl Adapter for Kitty {
    fn name(&self) -> &'static str {
        "kitty"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        env.app_config("kitty/kitty.conf").into_iter().collect()
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = self
            .resolved_config(env)
            .ok_or_else(|| anyhow!("kitty config not found"))?;
        let existing = std::fs::read_to_string(&path)?;
        let content = render_kitty(&existing, palette);
        let keys = content.1;
        Ok(Patched::single(path, content.0, keys))
    }
}

fn render_kitty(existing: &str, palette: &Palette) -> (String, Vec<String>) {
    let mut assignments = map();
    if let Some(fg) = color(palette, "foreground") {
        assignments.insert("foreground".into(), fg.clone());
        assignments.insert("cursor".into(), fg);
    }
    if let Some(bg) = color(palette, "background") {
        assignments.insert("cursor_text_color".into(), bg.clone());
        assignments.insert("background".into(), bg);
    }
    for i in 0..16u8 {
        if let Some(name) = ansi_name(i) {
            if let Some(hex) = color(palette, name) {
                assignments.insert(format!("color{i}"), hex);
            }
        }
    }
    apply_overrides(&mut assignments, palette, "kitty");

    patch_assignments(
        existing,
        &assignments,
        |line| leading_key(line).map(str::to_string),
        |k, v| format!("{k} {v}"),
        Insert::End,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn maps_canonical_to_colorn() {
        let existing = "# kitty config\nfont_family FiraCode\ncolor1 #000000\n";
        let (out, keys) = render_kitty(existing, &palette());
        assert!(out.contains("font_family FiraCode"));
        assert!(out.contains("foreground #eeeeee"));
        assert!(out.contains("background #111111"));
        assert!(out.contains("color0 #000000"));
        assert!(out.contains("color1 #ff0000"));
        assert!(out.contains("color15 #ffffff"));
        assert!(out.contains("cursor #eeeeee"));
        assert!(keys.contains(&"color1".to_string()));
    }

    #[test]
    fn preserves_unknown_keys_byte_for_byte() {
        let existing = "font_size 12.5\nmap ctrl+c copy\n# color0 #deadbe\n";
        let (out, _) = render_kitty(existing, &palette());
        assert!(out.contains("font_size 12.5"));
        assert!(out.contains("map ctrl+c copy"));
        assert!(out.contains("# color0 #deadbe"));
    }
}
