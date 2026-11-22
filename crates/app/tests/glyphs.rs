//! Every character the UI writes must exist in the fonts the app ships (egui's defaults: Ubuntu, Noto
//! Emoji, the emoji icon font, Hack) – a missing one is drawn as an empty box (we had ▾ and ✕).
//! Scans the string literals of the app's sources.

use ab_glyph::Font;
use std::collections::BTreeMap;

/// The fonts of the proportional family, which the UI's text uses (the monospace Hack font is only
/// for code and numbers, so a character only it has still shows as a box in labels).
fn ui_fonts() -> Vec<ab_glyph::FontRef<'static>> {
    let defs: &'static egui::FontDefinitions = Box::leak(Box::new(egui::FontDefinitions::default()));
    defs.families[&egui::FontFamily::Proportional].iter().filter_map(|name| defs.font_data.get(name)).filter_map(|d| ab_glyph::FontRef::try_from_slice_and_index(&d.font, d.index).ok()).collect()
}

fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The characters inside "…" string literals of `code` (comments skipped), with a line number each.
fn literal_chars(code: &str) -> Vec<(char, usize)> {
    let mut out = Vec::new();
    for (ln, line) in code.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let mut inside = false;
        let mut esc = false;
        let mut prev = ' ';
        for c in line.chars() {
            if !inside && c == '/' && prev == '/' {
                break;
            }
            if inside {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    inside = false;
                } else if !c.is_ascii() {
                    out.push((c, ln + 1));
                }
            } else if c == '"' && prev != '\'' {
                inside = true;
            }
            prev = c;
        }
    }
    out
}

#[test]
fn every_ui_character_is_in_the_fonts() {
    let fonts = ui_fonts();
    assert!(fonts.len() >= 3, "egui's fonts load");
    let covered = |c: char| fonts.iter().any(|f| f.glyph_id(c).0 != 0);
    let mut files = Vec::new();
    sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut missing: BTreeMap<char, Vec<String>> = BTreeMap::new();
    for f in &files {
        let code = std::fs::read_to_string(f).unwrap();
        // Tests may use any character in their own strings.
        let code = code.split("#[cfg(test)]").next().unwrap_or("");
        for (c, ln) in literal_chars(code) {
            // Variation selectors and joiners are not drawn on their own.
            if matches!(c, '\u{FE0F}' | '\u{FE0E}' | '\u{200D}') || covered(c) {
                continue;
            }
            missing.entry(c).or_default().push(format!("{}:{ln}", f.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display()));
        }
    }
    let report: Vec<String> = missing.iter().map(|(c, at)| format!("{c:?} U+{:04X} at {}", *c as u32, at.iter().take(3).cloned().collect::<Vec<_>>().join(", "))).collect();
    assert!(report.is_empty(), "characters the fonts cannot draw (shown as boxes):\n{}", report.join("\n"));
}

/// The check itself: ▾ and ✕ were drawn as boxes in the app, so they must count as missing.
#[test]
fn the_check_catches_known_missing_characters() {
    let fonts = ui_fonts();
    let covered = |c: char| fonts.iter().any(|f| f.glyph_id(c).0 != 0);
    assert!(covered('A') && covered('…') && covered('▶'));
    for c in ['▾', '✕'] {
        assert!(!covered(c), "{c} renders as a box in the app, the check must see that");
    }
    assert_eq!(literal_chars(r#"let x = "a▾b"; // ✕"#).iter().map(|c| c.0).collect::<String>(), "▾");
}
