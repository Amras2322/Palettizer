use super::{color, Adapter, FilePatch, Patched};
use crate::env::Env;
use crate::palette::Palette;
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::path::PathBuf;

/// micro — `~/.config/micro/settings.json` plus a generated colorscheme.
///
/// The plan described micro as storing colors directly in `settings.json`.
/// In reality micro loads colors from `colorschemes/<name>.micro`, so this
/// adapter writes a real colorscheme (the only way to make micro actually show
/// the palette) and points `settings.json` at it via `"colorscheme"`. All other
/// settings keys are preserved.
pub struct Micro;

const SCHEME_NAME: &str = "palettizer";

impl Adapter for Micro {
    fn name(&self) -> &'static str {
        "micro"
    }

    fn config_candidates(&self, env: &Env) -> Vec<PathBuf> {
        env.app_config("micro/settings.json").into_iter().collect()
    }

    fn patch(&self, env: &Env, palette: &Palette) -> Result<Patched> {
        let settings_path = self
            .resolved_config(env)
            .ok_or_else(|| anyhow!("micro settings.json not found"))?;
        let settings = std::fs::read_to_string(&settings_path)?;
        let new_settings = set_colorscheme(&settings)?;

        let scheme_dir = env
            .app_config("micro/colorschemes")
            .ok_or_else(|| anyhow!("cannot resolve micro config directory"))?;
        let scheme_path = scheme_dir.join(format!("{SCHEME_NAME}.micro"));
        let scheme = render_colorscheme(palette);

        Ok(Patched {
            files: vec![
                FilePatch {
                    path: settings_path,
                    content: new_settings,
                },
                FilePatch {
                    path: scheme_path,
                    content: scheme,
                },
            ],
            keys_touched: vec![
                "colorscheme".to_string(),
                format!("colorschemes/{SCHEME_NAME}.micro"),
            ],
        })
    }
}

fn set_colorscheme(existing: &str) -> Result<String> {
    let trimmed = existing.trim();
    let mut root: Value = if trimmed.is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(trimmed).context("micro settings.json is not valid JSON")?
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("micro settings.json is not a JSON object"))?;
    obj.insert("colorscheme".into(), Value::String(SCHEME_NAME.into()));
    let mut out = serde_json::to_string_pretty(&root)?;
    out.push('\n');
    Ok(out)
}

/// Build a `.micro` colorscheme mapping common syntax groups to the palette.
fn render_colorscheme(palette: &Palette) -> String {
    let fg = color(palette, "foreground").unwrap_or_else(|| "#ffffff".into());
    let bg = color(palette, "background").unwrap_or_else(|| "#000000".into());
    let get = |k: &str, d: &str| color(palette, k).unwrap_or_else(|| d.to_string());

    let mut lines: Vec<String> = vec![
        format!("color-link default \"{fg},{bg}\""),
        format!("color-link comment \"{}\"", get("brightBlack", "#888888")),
        format!("color-link constant \"{}\"", get("magenta", "#ff00ff")),
        format!("color-link identifier \"{}\"", get("cyan", "#00ffff")),
        format!("color-link statement \"{}\"", get("red", "#ff0000")),
        format!("color-link preproc \"{}\"", get("yellow", "#ffff00")),
        format!("color-link type \"{}\"", get("blue", "#0000ff")),
        format!("color-link special \"{}\"", get("brightMagenta", "#ff55ff")),
        format!("color-link underlined \"{}\"", get("blue", "#0000ff")),
        format!("color-link error \"{}\"", get("brightRed", "#ff5555")),
        format!("color-link todo \"{}\"", get("brightYellow", "#ffff55")),
        format!("color-link matchbrace \"{}\"", get("blue", "#0000ff")),
        format!(
            "color-link statusline \"{},{}\"",
            get("foreground", "#ffffff"),
            get("background", "#000000")
        ),
        // Extended identifier groups: micro falls back to the root `identifier`
        // when a subgroup is absent, but defining them enables sub-highlighting.
        format!("color-link identifier.class \"{}\"", get("cyan", "#00ffff")),
        format!("color-link identifier.var \"{}\"", get("cyan", "#00ffff")),
        // Extended constant groups.
        format!("color-link constant.string \"{}\"", get("green", "#00ff00")),
        format!(
            "color-link constant.number \"{}\"",
            get("yellow", "#ffff00")
        ),
        // Symbol groups.
        format!("color-link symbol \"{}\"", get("magenta", "#ff00ff")),
        format!(
            "color-link symbol.brackets \"{}\"",
            get("brightBlack", "#888888")
        ),
        format!("color-link symbol.tag \"{}\"", get("blue", "#0000ff")),
        // Type groups.
        format!("color-link type.keyword \"{}\"", get("yellow", "#ffff00")),
        // Git diff groups.
        format!("color-link diff-added \"{}\"", get("green", "#00ff00")),
        format!("color-link diff-modified \"{}\"", get("yellow", "#ffff00")),
        format!("color-link diff-deleted \"{}\"", get("red", "#ff0000")),
        // Gutter indicators (distinct from text `error`/`todo`).
        format!(
            "color-link gutter-error \"{}\"",
            get("brightRed", "#ff5555")
        ),
        format!(
            "color-link gutter-warning \"{}\"",
            get("brightYellow", "#ffff55")
        ),
        // UI groups.
        format!(
            "color-link tabbar \"{},{}\"",
            fg,
            get("background", "#000000")
        ),
        format!(
            "color-link indent-char \"{}\"",
            get("brightBlack", "#888888")
        ),
        format!(
            "color-link line-number \"{}\"",
            get("brightBlack", "#888888")
        ),
        format!(
            "color-link current-line-number \"{}\"",
            get("blue", "#0000ff")
        ),
        format!(
            "color-link cursor-line \"{},{}\"",
            get("background", "#000000"),
            fg
        ),
        format!(
            "color-link color-column \"{}\"",
            get("brightBlack", "#888888")
        ),
        // `default` is micro's special keyword for the terminal default color.
        "color-link type.extended \"default\"".to_string(),
    ];

    // app overrides are treated as extra `color-link <group> "#hex"` lines
    if let Some(overrides) = palette.overrides_for("micro") {
        for (group, hex) in overrides {
            if let Some(hex) = crate::palette::normalize_hex(hex) {
                lines.push(format!("color-link {group} \"{hex}\""));
            }
        }
    }

    let mut out = format!("# Generated by palettizer - do not edit\n# scheme: {SCHEME_NAME}\n");
    out.push_str(&lines.join("\n"));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testpalette::palette;

    #[test]
    fn sets_colorscheme_and_preserves_settings() {
        let existing = "{\n  \"tabsize\": 4,\n  \"colorscheme\": \"default\"\n}\n";
        let out = set_colorscheme(existing).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["colorscheme"], "palettizer");
        assert_eq!(v["tabsize"], 4);
    }

    #[test]
    fn empty_settings_becomes_object() {
        let out = set_colorscheme("").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["colorscheme"], "palettizer");
    }

    #[test]
    fn scheme_uses_palette_hexes() {
        let scheme = render_colorscheme(&palette());
        assert!(scheme.contains("#eeeeee,#111111"));
        assert!(scheme.contains("color-link comment"));
    }

    #[test]
    fn scheme_defines_extended_groups() {
        let scheme = render_colorscheme(&palette());
        let links = scheme.matches("color-link ").count();
        assert!(links >= 27, "expected 27+ color-link entries, got {links}");
        for group in [
            "identifier.class",
            "identifier.var",
            "constant.string",
            "constant.number",
            "symbol",
            "symbol.brackets",
            "symbol.tag",
            "type.keyword",
            "diff-added",
            "diff-modified",
            "diff-deleted",
            "gutter-error",
            "gutter-warning",
            "tabbar",
            "indent-char",
            "line-number",
            "current-line-number",
            "cursor-line",
            "color-column",
        ] {
            assert!(
                scheme.contains(&format!("color-link {group} ")),
                "missing color-link {group}"
            );
        }
        assert!(scheme.contains("color-link type.extended \"default\""));
    }
}
