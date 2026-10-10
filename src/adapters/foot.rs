use super::{apply_overrides, color, map, Adapter, Patched};
use crate::env::Env;
use crate::palette::{ansi_name, Palette};
use crate::textedit::patch_ini_section;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// foot — `~/.config/foot/foot.ini`, `[colors]` section.
///
/// Writes the modern `regularN`/`brightN` keys. If the existing file still uses
/// the deprecated `colorN` keys, those are updated in tandem so both schemes
/// stay consistent.
pub struct Foot;

impl Adapter for Foot {
    fn name(&self) -> &'static str {
        "foot"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        env.app_config("foot/foot.ini").into_iter().collect()
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = self
            .resolved_config(env)
            .ok_or_else(|| anyhow!("foot config not found"))?;
        let existing = std::fs::read_to_string(&path)?;
        let (content, keys) = render_foot(&existing, palette);
        Ok(Patched::single(path, content, keys))
    }
}

fn render_foot(existing: &str, palette: &Palette) -> (String, Vec<String>) {
    let mut assignments = map();
    if let Some(fg) = color(palette, "foreground") {
        assignments.insert("foreground".into(), fg.clone());
        assignments.insert("cursor".into(), fg);
    }
    if let Some(bg) = color(palette, "background") {
        assignments.insert("background".into(), bg);
    }
    for i in 0..16u8 {
        let Some(name) = ansi_name(i) else {
            continue;
        };
        let Some(hex) = color(palette, name) else {
            continue;
        };
        if i < 8 {
            assignments.insert(format!("regular{i}"), hex);
        } else {
            assignments.insert(format!("bright{}", i - 8), hex);
        }
    }
    if uses_legacy_colors(existing) {
        for i in 0..16u8 {
            if let Some(name) = ansi_name(i) {
                if let Some(hex) = color(palette, name) {
                    assignments.insert(format!("color{i}"), hex);
                }
            }
        }
    }
    apply_overrides(&mut assignments, palette, "foot");

    patch_ini_section(existing, "[colors]", &assignments)
}

/// Whether the `[colors]` section already contains any `colorN=` keys.
fn uses_legacy_colors(text: &str) -> bool {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.eq_ignore_ascii_case("[colors]");
            continue;
        }
        if in_section {
            if let Some(rest) = trimmed.strip_prefix("color") {
                if rest
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                    && rest.contains('=')
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn maps_regular_and_bright() {
        let existing = "[main]\nterm=xterm-256color\n\n[colors]\nforeground=#000000\n";
        let (out, _) = render_foot(existing, &palette());
        assert!(out.contains("[main]\nterm=xterm-256color"));
        assert!(out.contains("foreground=#eeeeee"));
        assert!(out.contains("background=#111111"));
        assert!(out.contains("regular0=#000000"));
        assert!(out.contains("regular7=#ffffff"));
        assert!(out.contains("bright0=#555555"));
        assert!(out.contains("bright7=#ffffff"));
    }

    #[test]
    fn keeps_legacy_color_keys_in_sync() {
        let existing = "[colors]\ncolor0=#111111\ncolor15=#222222\n";
        let (out, _) = render_foot(existing, &palette());
        assert!(out.contains("color0=#000000"));
        assert!(out.contains("color15=#ffffff"));
    }

    #[test]
    fn preserves_unrelated_sections() {
        let existing = "[main]\nfont=monospace:size=11\n[scrollback]\nlines=10000\n";
        let (out, _) = render_foot(existing, &palette());
        assert!(out.contains("font=monospace:size=11"));
        assert!(out.contains("lines=10000"));
    }
}
