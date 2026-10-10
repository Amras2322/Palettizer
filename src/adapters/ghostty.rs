use super::{color, Adapter, Patched};
use crate::env::Env;
use crate::palette::{ansi_name, Palette};
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// ghostty — `~/.config/ghostty/config`, falling back to
/// `~/.config/ghostty/theme.yaml`.
///
/// Both files use the same `key = value` directive syntax (including
/// `palette = N=#hex` lines), so one renderer covers both. When neither file
/// exists the app is reported as not compatible rather than guessed at.
pub struct Ghostty;

impl Adapter for Ghostty {
    fn name(&self) -> &'static str {
        "ghostty"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        ["ghostty/config", "ghostty/theme.yaml"]
            .iter()
            .filter_map(|p| env.app_config(p))
            .collect()
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = self
            .resolved_config(env)
            .ok_or_else(|| anyhow!("ghostty config not found"))?;
        let existing = std::fs::read_to_string(&path)?;
        let (content, keys) = render_ghostty(&existing, palette);
        Ok(Patched::single(path, content, keys))
    }
}

fn render_ghostty(existing: &str, palette: &Palette) -> (String, Vec<String>) {
    let trailing_newline = existing.ends_with('\n');
    let mut lines: Vec<String> = crate::textedit::split_lines(existing);

    // Simple directives.
    let mut simple: Vec<(String, String)> = Vec::new();
    if let Some(fg) = color(palette, "foreground") {
        simple.push(("foreground".into(), fg.clone()));
        simple.push(("cursor-color".into(), fg));
    }
    if let Some(bg) = color(palette, "background") {
        simple.push(("background".into(), bg));
    }
    // Palette entries.
    let mut palette_colors: Vec<(u8, String)> = Vec::new();
    for i in 0..16u8 {
        if let Some(name) = ansi_name(i) {
            if let Some(hex) = color(palette, name) {
                palette_colors.push((i, hex));
            }
        }
    }
    // Overrides: "paletteN" for an index, otherwise a raw directive.
    if let Some(overrides) = palette.overrides_for("ghostty") {
        for (key, raw) in overrides {
            let Some(hex) = crate::palette::normalize_hex(raw) else {
                continue;
            };
            if let Some(idx) = key
                .strip_prefix("palette")
                .and_then(|s| s.parse::<u8>().ok())
            {
                if let Some(slot) = palette_colors.iter_mut().find(|(i, _)| *i == idx) {
                    slot.1 = hex;
                } else {
                    palette_colors.push((idx, hex));
                }
            } else {
                simple.retain(|(k, _)| k != key);
                simple.push((key.clone(), hex));
            }
        }
    }

    let mut touched: Vec<String> = Vec::new();
    let mut simple_done: Vec<bool> = vec![false; simple.len()];
    let mut palette_done: Vec<bool> = vec![false; palette_colors.len()];

    for line in lines.iter_mut() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // palette = N=#hex
        if let Some(rest) = trimmed.strip_prefix("palette") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                if let Some((idx_str, _)) = rest.split_once('=') {
                    if let Ok(idx) = idx_str.trim().parse::<u8>() {
                        if let Some(pos) = palette_colors.iter().position(|(i, _)| *i == idx) {
                            line.clone_from(&format!("palette = {idx}={}", palette_colors[pos].1));
                            palette_done[pos] = true;
                            touched.push(format!("palette[{idx}]"));
                        }
                        continue;
                    }
                }
            }
            continue;
        }
        // simple key = value
        let key = trimmed
            .split(|c: char| c == '=' || c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_string();
        if let Some(pos) = simple.iter().position(|(k, _)| *k == key) {
            line.clone_from(&format!("{key} = {}", simple[pos].1));
            simple_done[pos] = true;
            touched.push(key);
        }
    }

    for (pos, (k, v)) in simple.iter().enumerate() {
        if !simple_done[pos] {
            lines.push(format!("{k} = {v}"));
            touched.push(k.clone());
        }
    }
    for (pos, (idx, hex)) in palette_colors.iter().enumerate() {
        if !palette_done[pos] {
            lines.push(format!("palette = {idx}={hex}"));
            touched.push(format!("palette[{idx}]"));
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    (out, touched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn patches_config_palette_and_simple_keys() {
        let existing = "# ghostty\nfont-family = Fira Code\nbackground = #000000\npalette = 1=#111111\npalette = 2=#222222\n";
        let (out, keys) = render_ghostty(existing, &palette());
        assert!(out.contains("# ghostty"));
        assert!(out.contains("font-family = Fira Code"));
        assert!(out.contains("background = #111111"));
        assert!(out.contains("foreground = #eeeeee"));
        assert!(out.contains("cursor-color = #eeeeee"));
        assert!(out.contains("palette = 1=#ff0000"));
        assert!(out.contains("palette = 2=#00ff00"));
        assert!(out.contains("palette = 0=#000000"));
        assert!(keys.contains(&"palette[1]".to_string()));
    }

    #[test]
    fn preserves_unknown_directives() {
        let existing = "window-padding-x = 4\nconfirm-close-surface = false\n";
        let (out, _) = render_ghostty(existing, &palette());
        assert!(out.contains("window-padding-x = 4"));
        assert!(out.contains("confirm-close-surface = false"));
    }
}
