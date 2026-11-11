//! The folder name of a new world (`WorldCreationUiState.findResultFolder`,
//! `FileUtil.findAvailableName`).

use std::path::Path;

/// `SharedConstants.ILLEGAL_FILE_CHARACTERS`.
const ILLEGAL_FILE_CHARACTERS: [char; 15] = [
    '/', '\n', '\r', '\t', '\0', '\u{c}', '`', '?', '*', '\\', '<', '>', '|', '"', ':',
];

/// The folder a new world named `name` gets under `saves`.
pub(super) fn result_folder(saves: &Path, name: &str) -> String {
    // String.trim drops every char up to U+0020.
    let trimmed = name.trim_matches(|c: char| c <= ' ');
    let wanted = if trimmed.is_empty() {
        "New World"
    } else {
        trimmed
    };
    // A failure (an empty or oversized copy counter) falls back to "World".
    find_available_name(saves, wanted)
        .or_else(|| find_available_name(saves, "World"))
        .unwrap_or_else(|| "World".to_owned())
}

/// `FileUtil.sanitizeName`.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if ILLEGAL_FILE_CHARACTERS.contains(&c) || matches!(c, '.' | '/' | '"') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// `FileUtil.isPathPartPortable`: not a reserved Windows device name.
fn is_path_part_portable(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "COM" | "CLOCK$" | "CON" | "PRN" | "AUX" | "NUL"
    ) || (upper.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    !reserved
}

/// `FileUtil.COPY_COUNTER_PATTERN` (`(?<name>.*) \((?<count>\d*)\)`) over the whole string:
/// the name and the digits of the trailing ` (n)`.
fn split_copy_counter(name: &str) -> Option<(&str, &str)> {
    // `.` does not match these line terminators.
    if name.contains(['\u{85}', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let inner = name.strip_suffix(')')?;
    let open = inner.rfind(" (")?;
    let digits = &inner[open + 2..];
    digits
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then_some((&inner[..open], digits))
}

/// Cuts to at most `max` UTF-16 units, as `String.substring` does (never inside a pair here).
fn cut_utf16(s: &str, max: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        units += c.len_utf16();
        if units > max {
            break;
        }
        out.push(c);
    }
    out
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `FileUtil.findAvailableName(saves, name, "")`; `None` where vanilla throws.
fn find_available_name(saves: &Path, name: &str) -> Option<String> {
    let mut base = sanitize_name(name);
    if !is_path_part_portable(&base) {
        base = format!("_{base}_");
    }
    let mut count: i32 = 0;
    if let Some((stem, digits)) = split_copy_counter(&base) {
        // Integer.parseInt throws on an empty or oversized count.
        let parsed = digits.parse::<i32>().ok()?;
        let stem = stem.to_owned();
        base = stem;
        count = parsed;
    }
    if utf16_len(&base) > 255 {
        base = cut_utf16(&base, 255);
    }
    loop {
        let mut candidate = base.clone();
        if count != 0 {
            let suffix = format!(" ({count})");
            candidate = cut_utf16(&candidate, 255 - suffix.len());
            candidate.push_str(&suffix);
        }
        if !saves.join(&candidate).exists() {
            return Some(candidate);
        }
        count = count.checked_add(1)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(taken: &[&str], name: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "hz-world-folder-{}-{:x}",
            std::process::id(),
            taken.len() * 31 + name.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for t in taken {
            std::fs::create_dir(dir.join(t)).unwrap();
        }
        let result = result_folder(&dir, name);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn a_new_worlds_folder_is_named_as_vanilla_names_it() {
        assert_eq!(folder(&[], ""), "New World");
        assert_eq!(folder(&[], "  \t "), "New World");
        assert_eq!(folder(&[], "  Bob's world!  "), "Bob's world!");
        assert_eq!(folder(&[], "a/b.c\"d:e"), "a_b_c_d_e");
        assert_eq!(folder(&[], "con"), "_con_");
        assert_eq!(folder(&[], "LPT3"), "_LPT3_");
        assert_eq!(folder(&[], "COM10"), "COM10");
        assert_eq!(folder(&["Earth"], "Earth"), "Earth (1)");
        assert_eq!(folder(&["Earth", "Earth (1)"], "Earth"), "Earth (2)");
        // A name that ends in a copy counter continues from it.
        assert_eq!(folder(&[], "Earth (3)"), "Earth (3)");
        assert_eq!(folder(&["Earth (3)"], "Earth (3)"), "Earth (4)");
        assert_eq!(folder(&[], "Earth (0)"), "Earth");
        // An empty counter makes vanilla throw: the fallback is "World".
        assert_eq!(folder(&[], "Earth ()"), "World");
        assert_eq!(folder(&["World"], "Earth ()"), "World (1)");
        assert_eq!(folder(&[], &"x".repeat(300)), "x".repeat(255));
        assert_eq!(
            folder(&["y".repeat(255).as_str()], &"y".repeat(300)),
            format!("{} (1)", "y".repeat(251))
        );
    }
}
