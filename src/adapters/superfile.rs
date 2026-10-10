use super::{color, Adapter, Patched};
use crate::detect::DetectResult;
use crate::env::Env;
use crate::palette::{normalize_hex, Palette};
use crate::textedit::split_lines;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// superfile — `~/.config/superfile/config.yaml`.
///
/// The plan specifies a nested `color:` mapping. This adapter edits only the
/// color keys inside that mapping **textually**, so comments, anchors and every
/// other key in the file survive byte-for-byte. A `config.toml`-only install is
/// reported as unsupported rather than guessed.
pub struct Superfile;

impl Adapter for Superfile {
    fn name(&self) -> &'static str {
        "superfile"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        ["superfile/config.yaml", "superfile/config.toml"]
            .iter()
            .filter_map(|p| env.app_config(p))
            .collect()
    }

    fn detect(&self, env: &Env) -> DetectResult {
        let Some(binary) = env
            .find_in_path("superfile")
            .or_else(|| env.find_in_path("spf"))
        else {
            return DetectResult::not_installed("superfile");
        };
        if let Some(p) = env
            .app_config("superfile/config.yaml")
            .filter(|p| p.exists())
        {
            return DetectResult::compatible(binary, p);
        }
        if let Some(p) = env
            .app_config("superfile/config.toml")
            .filter(|p| p.exists())
        {
            return DetectResult::unsupported(
                binary,
                p,
                "config.toml schema not yet supported; only config.yaml is patched".to_string(),
            );
        }
        DetectResult::config_missing(binary, "no superfile config.yaml found".into())
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = env
            .app_config("superfile/config.yaml")
            .filter(|p| p.exists())
            .ok_or_else(|| anyhow!("superfile config.yaml not found"))?;
        let existing = std::fs::read_to_string(&path)?;
        let (content, keys) = render_superfile(&existing, palette);
        Ok(Patched::single(path, content, keys))
    }
}

/// Canonical palette key -> superfile color key.
const MAPPING: &[(&str, &str)] = &[
    ("background", "main_bg"),
    ("foreground", "main_fg"),
    ("background", "full_screen_bg"),
    ("foreground", "full_screen_fg"),
    ("background", "panel_bg"),
    ("foreground", "panel_fg"),
    ("brightBlack", "file_panel_border"),
    ("brightBlack", "sidebar_border"),
    ("blue", "file_panel_selected_bg"),
    ("background", "file_panel_selected_fg"),
];

fn render_superfile(existing: &str, palette: &Palette) -> (String, Vec<String>) {
    let mut desired: Vec<(String, String)> = Vec::new();
    for (canonical, target) in MAPPING {
        if let Some(hex) = color(palette, canonical) {
            if !desired.iter().any(|(k, _)| k == target) {
                desired.push(((*target).to_string(), hex));
            }
        }
    }
    if let Some(overrides) = palette.overrides_for("superfile") {
        for (key, raw) in overrides {
            if let Some(hex) = normalize_hex(raw) {
                desired.retain(|(k, _)| k != key);
                desired.push((key.clone(), hex));
            }
        }
    }
    patch_color_block(existing, &desired)
}

/// Insert or update `key: "hex"` child lines under the top-level `color:` key,
/// leaving comments and unrelated lines untouched.
fn patch_color_block(existing: &str, desired: &[(String, String)]) -> (String, Vec<String>) {
    let trailing_newline = existing.ends_with('\n');
    let mut lines = split_lines(existing);
    let mut touched: Vec<String> = Vec::new();

    match lines.iter().position(|l| is_top_level_key(l, "color")) {
        Some(h) => {
            lines[h] = "color:".to_string();
            let end = lines
                .iter()
                .enumerate()
                .skip(h + 1)
                .find(|(_, l)| is_top_level_boundary(l))
                .map(|(i, _)| i)
                .unwrap_or(lines.len());

            let mut remaining = desired.to_vec();
            for line in lines.iter_mut().take(end).skip(h + 1) {
                if let Some(key) = yaml_child_key(line) {
                    if let Some(pos) = remaining.iter().position(|(k, _)| *k == key) {
                        let hex = remaining.remove(pos).1;
                        *line = format!("  {key}: \"{hex}\"");
                        touched.push(key);
                    }
                }
            }
            let insert_at = (h + 1..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .map(|i| i + 1)
                .unwrap_or(h + 1);
            for (offset, (key, hex)) in remaining.into_iter().enumerate() {
                lines.insert(insert_at + offset, format!("  {key}: \"{hex}\""));
                touched.push(key);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push("color:".to_string());
            for (key, hex) in desired {
                lines.push(format!("  {key}: \"{hex}\""));
                touched.push(key.clone());
            }
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    (out, touched)
}

/// True when `line` is an unindented `<key>:` mapping entry.
fn is_top_level_key(line: &str, key: &str) -> bool {
    if line.is_empty() || line.starts_with([' ', '\t']) {
        return false;
    }
    let trimmed = line.trim_end();
    if trimmed.starts_with('#') {
        return false;
    }
    match trimmed.split_once(':') {
        Some((k, _)) => k.trim() == key,
        None => false,
    }
}

/// True when `line` begins a new top-level entry (a non-indented, non-comment,
/// non-blank line), which ends the `color:` block.
fn is_top_level_boundary(line: &str) -> bool {
    if line.trim().is_empty() || line.starts_with([' ', '\t']) {
        return false;
    }
    !line.trim_start().starts_with('#')
}

/// Extract the key of an indented `key: value` child line.
fn yaml_child_key(line: &str) -> Option<String> {
    if line.is_empty() || !line.starts_with([' ', '\t']) {
        return None;
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let (k, _) = trimmed.split_once(':')?;
    let k = k.trim();
    if k.is_empty() {
        None
    } else {
        Some(k.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn preserves_other_keys_and_sets_colors() {
        let existing = "file_panel_width: 50\ncolor:\n  main_fg: \"#123456\"\n";
        let (out, keys) = render_superfile(existing, &palette());
        assert!(out.contains("file_panel_width: 50"));
        assert!(out.contains("main_fg: \"#eeeeee\""));
        assert!(out.contains("main_bg: \"#111111\""));
        assert!(keys.contains(&"main_fg".to_string()));
    }

    #[test]
    fn creates_color_mapping_when_absent() {
        let existing = "file_panel_width: 50\n";
        let (out, _) = render_superfile(existing, &palette());
        assert!(out.contains("file_panel_width: 50"));
        assert!(out.contains("color:"));
        assert!(out.contains("  main_bg: \"#111111\""));
    }

    #[test]
    fn preserves_comments_and_other_color_keys() {
        let existing = "# top comment\ncolor:\n  # border color\n  main_fg: \"#123456\" # trailing\n  custom_key: \"#abcdef\"\n\nother: 1\n";
        let (out, _) = render_superfile(existing, &palette());
        assert!(out.contains("# top comment"));
        assert!(out.contains("# border color"));
        assert!(out.contains("custom_key: \"#abcdef\""));
        assert!(out.contains("main_fg: \"#eeeeee\""));
        assert!(out.contains("\nother: 1"));
        // color keys must stay inside the color block, before `other:`.
        let other = out.find("other: 1").unwrap();
        let bg = out.find("main_bg").unwrap();
        assert!(bg < other);
    }

    #[test]
    fn inline_color_header_is_normalized() {
        let existing = "color: {}\n";
        let (out, _) = render_superfile(existing, &palette());
        assert!(out.contains("color:"));
        assert!(out.contains("  main_bg: \"#111111\""));
    }
}
