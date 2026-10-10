use super::{color, Adapter, Patched};
use crate::env::Env;
use crate::palette::{ansi_name, Palette};
use crate::textedit::split_lines;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// tmux — `~/.tmux.conf` (or `~/.config/tmux/tmux.conf`).
///
/// tmux has no canonical palette key scheme, so this adapter is template-driven:
/// it manages a single delimited block marked with `palettizer` sentinels and
/// replaces only that block on re-apply. Everything outside the block is left
/// byte-for-byte. Palette values are also exported as `@palettizer_<name>`
/// user options for tmux theme frameworks.
pub struct Tmux;

const START: &str = "# >>> palettizer >>>";
const END: &str = "# <<< palettizer <<<";

impl Adapter for Tmux {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(p) = env.app_config("~/.tmux.conf") {
            candidates.push(p);
        }
        if let Some(p) = env.app_config("tmux/tmux.conf") {
            candidates.push(p);
        }
        candidates
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let path = self
            .resolved_config(env)
            .ok_or_else(|| anyhow!("tmux config not found"))?;
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let (content, keys) = render_tmux(&existing, palette);
        Ok(Patched::single(path, content, keys))
    }
}

fn render_tmux(existing: &str, palette: &Palette) -> (String, Vec<String>) {
    let bg = color(palette, "background").unwrap_or_else(|| "#000000".into());
    let fg = color(palette, "foreground").unwrap_or_else(|| "#ffffff".into());
    let blue = color(palette, "blue").unwrap_or_else(|| "#0000ff".into());
    let bright_black = color(palette, "brightBlack").unwrap_or_else(|| "#555555".into());
    let get = |k: &str, d: &str| color(palette, k).unwrap_or_else(|| d.to_string());

    let mut block: Vec<String> = vec![START.to_string()];
    block.push(format!("set -g status-style \"bg={bg},fg={fg}\""));
    block.push(format!("set -g message-style \"bg={blue},fg={fg}\""));
    block.push(format!(
        "set -g message-command-style \"bg={bg},fg={blue}\""
    ));
    block.push(format!("set -g pane-border-style \"fg={bright_black}\""));
    block.push(format!("set -g pane-active-border-style \"fg={blue}\""));
    block.push(format!("set -g mode-style \"bg={bright_black},fg={fg}\""));
    block.push(format!("set -g clock-mode-colour \"{blue}\""));
    block.push(format!("set -g status-left-style \"bg={bg},fg={fg}\""));
    block.push(format!("set -g status-right-style \"bg={bg},fg={fg}\""));
    block.push(format!("set -g window-status-style \"bg={bg},fg={fg}\""));
    block.push(format!(
        "set -g window-status-current-style \"bg={blue},fg={bg}\""
    ));

    let mut touched: Vec<String> = vec![
        "status-style".into(),
        "message-style".into(),
        "pane-border-style".into(),
        "pane-active-border-style".into(),
        "window-status-current-style".into(),
    ];

    // Export the raw palette for tmux theme frameworks.
    for i in 0..16u8 {
        if let Some(name) = ansi_name(i) {
            if let Some(hex) = color(palette, name) {
                block.push(format!("set -g @palettizer_{name} \"{hex}\""));
                touched.push(format!("@palettizer_{name}"));
            }
        }
    }
    for key in ["foreground", "background"] {
        let hex = get(key, "#000000");
        block.push(format!("set -g @palettizer_{key} \"{hex}\""));
        touched.push(format!("@palettizer_{key}"));
    }

    // Overrides become `@palettizer_<key>` user options (safe under any tmux).
    if let Some(overrides) = palette.overrides_for("tmux") {
        for (key, raw) in overrides {
            if let Some(hex) = crate::palette::normalize_hex(raw) {
                let name = key.strip_prefix('@').unwrap_or(key);
                block.push(format!("set -g @palettizer_{name} \"{hex}\""));
                touched.push(format!("@palettizer_{name}"));
            }
        }
    }
    block.push(END.to_string());

    let out = upsert_block(existing, &block);
    (out, touched)
}

/// Replace the sentinel-delimited block in `existing`, or append it.
///
/// Only a *balanced* pair (a `START` followed later by an `END`) is treated as
/// a block to replace. Every other sentinel-only line is dropped so a
/// half-deleted block self-heals; user lines are never removed.
fn upsert_block(existing: &str, block: &[String]) -> String {
    let trailing_newline = existing.is_empty() || existing.ends_with('\n');
    let lines = split_lines(existing);

    // First balanced START..=END pair, if any.
    let pair = lines.iter().position(|l| l.trim() == START).and_then(|s| {
        lines
            .iter()
            .enumerate()
            .skip(s + 1)
            .find(|(_, l)| l.trim() == END)
            .map(|(e, _)| (s, e))
    });

    // Rebuild without any sentinel lines or the matched block body, recording
    // where the fresh block should be inserted (the first sentinel's position).
    let mut cleaned: Vec<String> = Vec::with_capacity(lines.len() + block.len());
    let mut insert_at: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((s, e)) = pair {
            if i >= s && i <= e {
                if i == s {
                    insert_at = Some(cleaned.len());
                }
                continue;
            }
        }
        let trimmed = line.trim();
        if trimmed == START || trimmed == END {
            if insert_at.is_none() {
                insert_at = Some(cleaned.len());
            }
            continue;
        }
        cleaned.push(line.clone());
    }

    match insert_at {
        Some(pos) => {
            for (offset, b) in block.iter().enumerate() {
                cleaned.insert(pos + offset, b.clone());
            }
        }
        None => {
            if !cleaned.is_empty() && !cleaned.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
                cleaned.push(String::new());
            }
            cleaned.extend(block.iter().cloned());
        }
    }

    let mut out = cleaned.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn appends_block_preserving_user_config() {
        let existing = "set -g prefix C-a\nbind r source-file ~/.tmux.conf\n";
        let (out, _) = render_tmux(existing, &palette());
        assert!(out.contains("set -g prefix C-a"));
        assert!(out.contains("bind r source-file ~/.tmux.conf"));
        assert!(out.contains(START));
        assert!(out.contains(END));
        assert!(out.contains("set -g @palettizer_blue \"#0000ff\""));
    }

    #[test]
    fn reapplying_replaces_block_without_duplicating() {
        let existing = "set -g prefix C-a\n";
        let (once, _) = render_tmux(existing, &palette());
        let (twice, _) = render_tmux(&once, &palette());
        assert_eq!(twice.matches(START).count(), 1);
        assert_eq!(twice.matches(END).count(), 1);
        assert!(twice.contains("set -g prefix C-a"));
    }

    #[test]
    fn empty_config_gets_block() {
        let (out, _) = render_tmux("", &palette());
        assert!(out.starts_with(START));
        assert!(out.trim_end().ends_with(END));
    }

    #[test]
    fn orphan_start_marker_does_not_delete_user_lines() {
        // A START with no matching END, followed by user config, must self-heal
        // without consuming the user's lines on the following apply.
        let corrupt = format!(
            "set -g prefix C-a\n{START}\nbind r source-file ~/.tmux.conf\nset -g @palettizer_x \"#ffffff\"\n"
        );
        let (once, _) = render_tmux(&corrupt, &palette());
        let (twice, _) = render_tmux(&once, &palette());
        assert!(twice.contains("set -g prefix C-a"));
        assert!(twice.contains("bind r source-file ~/.tmux.conf"));
        assert!(twice.contains("set -g @palettizer_x \"#ffffff\""));
        assert_eq!(twice.matches(START).count(), 1);
        assert_eq!(twice.matches(END).count(), 1);
    }
}
