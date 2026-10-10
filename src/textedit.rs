use indexmap::{IndexMap, IndexSet};

/// Where to place assignments whose keys were not already present.
pub enum Insert {
    /// Append to the end of the file.
    End,
    /// Insert immediately after the given section header (creating the header
    /// at the end if it is missing).
    UnderSection(String),
}

/// Replace existing `key ... value` lines in place and append/insert any keys
/// that were not found. Every line that does not define a requested key is
/// copied through byte-for-byte, preserving user customisations.
///
/// * `key_of` extracts the assignment key from a line (`None` for comments,
///   blanks and non-assignment lines).
/// * `render` produces the replacement line for a key/value pair.
///
/// Returns the new text and the keys that were touched.
pub fn patch_assignments<F, G>(
    text: &str,
    assignments: &IndexMap<String, String>,
    key_of: F,
    render: G,
    insert: Insert,
) -> (String, Vec<String>)
where
    F: Fn(&str) -> Option<String>,
    G: Fn(&str, &str) -> String,
{
    let trailing_newline = text.ends_with('\n');
    let mut lines: Vec<String> = split_lines(text);

    let mut remaining: IndexSet<String> = assignments.keys().cloned().collect();
    let mut touched: Vec<String> = Vec::new();

    for line in lines.iter_mut() {
        if let Some(k) = key_of(line) {
            if let Some((_, v)) = assignments.get_key_value(&k) {
                // Only replace the first occurrence; leave later duplicates as-is.
                if remaining.shift_remove(&k) {
                    *line = render(&k, v);
                    touched.push(k);
                }
            }
        }
    }

    if !remaining.is_empty() {
        let new_lines: Vec<String> = remaining
            .iter()
            .map(|k| {
                touched.push(k.clone());
                render(k, assignments.get(k).expect("remaining key present"))
            })
            .collect();
        match &insert {
            Insert::End => lines.extend(new_lines),
            Insert::UnderSection(section) => insert_under_section(&mut lines, section, new_lines),
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    (out, touched)
}

/// Patch an INI file, scoping replacements to a single `[section]`. Keys that
/// are absent from the section are added at its end. Content outside the
/// section is preserved byte-for-byte.
pub fn patch_ini_section(
    text: &str,
    section: &str,
    assignments: &IndexMap<String, String>,
) -> (String, Vec<String>) {
    let trailing_newline = text.ends_with('\n');
    let mut lines = split_lines(text);

    let header_idx = lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(section));

    let (start, end) = match header_idx {
        Some(i) => {
            let end = lines
                .iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, l)| l.trim_start().starts_with('['))
                .map(|(idx, _)| idx)
                .unwrap_or(lines.len());
            (i + 1, end)
        }
        None => (lines.len(), lines.len()),
    };

    let mut remaining: IndexSet<String> = assignments.keys().cloned().collect();
    let mut touched: Vec<String> = Vec::new();

    for line in lines.iter_mut().take(end).skip(start) {
        if let Some(k) = ini_key(line).map(str::to_string) {
            if let Some((_, v)) = assignments.get_key_value(&k) {
                if remaining.shift_remove(&k) {
                    *line = format!("{k}={v}");
                    touched.push(k);
                }
            }
        }
    }

    if !remaining.is_empty() {
        let new_lines: Vec<String> = remaining
            .iter()
            .map(|k| {
                touched.push(k.clone());
                format!("{k}={}", assignments.get(k).expect("remaining key present"))
            })
            .collect();
        if header_idx.is_some() {
            // Insert before the first blank line at the end of the section, or
            // simply at the section end.
            let insert_at = lines[start..end]
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map(|rel| start + rel + 1)
                .unwrap_or(start);
            for (offset, nl) in new_lines.into_iter().enumerate() {
                lines.insert(insert_at + offset, nl);
            }
        } else {
            if !lines.is_empty() && !lines.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(section.to_string());
            lines.extend(new_lines);
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    (out, touched)
}

/// Split text into lines without a trailing empty element.
pub fn split_lines(text: &str) -> Vec<String> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').map(str::to_string).collect()
    }
}

/// Return the first whitespace/`=`/`:` delimited token of a line, unless the
/// line is blank or a comment.
pub fn leading_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let end = trimmed
        .find(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .unwrap_or(trimmed.len());
    let token = trimmed[..end].trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Return the token before `=` on a line, or `None` for comments/blanks.
pub fn ini_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn insert_under_section(lines: &mut Vec<String>, section: &str, new_lines: Vec<String>) {
    let idx = lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(section));
    match idx {
        Some(i) => {
            let at = i + 1;
            for (offset, nl) in new_lines.into_iter().enumerate() {
                lines.insert(at + offset, nl);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().map(|l| l.trim().is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(section.to_string());
            lines.extend(new_lines);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kitty_key(line: &str) -> Option<String> {
        leading_key(line).map(str::to_string)
    }

    #[test]
    fn replaces_in_place_and_preserves_other_lines() {
        let text = "# my theme\nforeground #aaa\nfont_family FiraCode\ncolor1 #111\n";
        let mut a = IndexMap::new();
        a.insert("foreground".into(), "#eeeeee".into());
        a.insert("color1".into(), "#ff0000".into());
        let (out, touched) =
            patch_assignments(text, &a, kitty_key, |k, v| format!("{k} {v}"), Insert::End);
        assert!(out.contains("# my theme"));
        assert!(out.contains("font_family FiraCode"));
        assert!(out.contains("foreground #eeeeee"));
        assert!(out.contains("color1 #ff0000"));
        assert_eq!(touched, vec!["foreground", "color1"]);
    }

    #[test]
    fn appends_missing_keys() {
        let text = "foreground #aaa\n";
        let mut a = IndexMap::new();
        a.insert("background".into(), "#111111".into());
        let (out, touched) =
            patch_assignments(text, &a, kitty_key, |k, v| format!("{k} {v}"), Insert::End);
        assert!(out.ends_with("background #111111\n"));
        assert_eq!(touched, vec!["background"]);
    }

    #[test]
    fn inserts_under_section() {
        let text = "[main]\nfoo=1\n[colors]\nred=#000\n";
        let mut a = IndexMap::new();
        a.insert("background".into(), "#111111".into());
        let (out, _) = patch_assignments(
            text,
            &a,
            |l| ini_key(l).map(str::to_string),
            |k, v| format!("{k}={v}"),
            Insert::UnderSection("[colors]".into()),
        );
        let colors_pos = out.find("[colors]").unwrap();
        let bg_pos = out.find("background").unwrap();
        let main_pos = out.find("[main]").unwrap();
        assert!(main_pos < colors_pos);
        assert!(colors_pos < bg_pos);
    }

    #[test]
    fn duplicate_keys_only_first_is_replaced() {
        // A duplicated palette key must not be silently rewritten twice; the
        // first occurrence is updated and later duplicates are preserved.
        let text = "color1 #111111\ncolor1 #222222\n";
        let mut a = IndexMap::new();
        a.insert("color1".into(), "#ff0000".into());
        let (out, touched) =
            patch_assignments(text, &a, kitty_key, |k, v| format!("{k} {v}"), Insert::End);
        assert!(out.contains("color1 #ff0000"));
        assert!(out.contains("color1 #222222"));
        assert_eq!(touched, vec!["color1"]);
    }

    #[test]
    fn commented_key_is_ignored() {
        let text = "# color1 #999\n";
        let mut a = IndexMap::new();
        a.insert("color1".into(), "#ff0000".into());
        let (out, touched) =
            patch_assignments(text, &a, kitty_key, |k, v| format!("{k} {v}"), Insert::End);
        assert!(out.contains("# color1 #999"));
        assert!(out.trim_end().ends_with("color1 #ff0000"));
        assert_eq!(touched, vec!["color1"]);
    }

    #[test]
    fn ini_section_scoping() {
        let text = "[main]\nforeground = keepme\n[colors]\nforeground=#000\nbackground=#fff\n[other]\nforeground=untouched\n";
        let mut a = IndexMap::new();
        a.insert("foreground".into(), "#eeeeee".into());
        a.insert("regular0".into(), "#000000".into());
        let (out, touched) = patch_ini_section(text, "[colors]", &a);
        assert!(out.contains("[main]\nforeground = keepme"));
        assert!(out.contains("[other]\nforeground=untouched"));
        assert!(out.contains("[colors]\nforeground=#eeeeee"));
        assert!(out.contains("regular0=#000000"));
        assert_eq!(touched, vec!["foreground", "regular0"]);
    }

    #[test]
    fn ini_missing_section_is_created() {
        let text = "[main]\nfoo=1\n";
        let mut a = IndexMap::new();
        a.insert("foreground".into(), "#eeeeee".into());
        let (out, _) = patch_ini_section(text, "[colors]", &a);
        assert!(out.contains("[main]\nfoo=1"));
        assert!(out.contains("[colors]\nforeground=#eeeeee"));
    }
}
