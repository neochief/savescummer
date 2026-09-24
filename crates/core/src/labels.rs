//! Checkpoint labels: one line, at most 100 characters, enforced by the host
//! for every client.

pub const MAX_LABEL_CHARS: usize = 100;

/// Normalizes a label: line breaks become spaces, the ends are trimmed and
/// the length is capped. Empty means no label.
pub fn normalize(label: &str) -> Option<String> {
    let one_line: String =
        label.replace("\r\n", " ").chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    let trimmed = one_line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let capped: String = trimmed.chars().take(MAX_LABEL_CHARS).collect();
    Some(capped.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_joins_lines() {
        assert_eq!(normalize("  Before boss\r\nfight \n"), Some("Before boss fight".into()));
        assert_eq!(normalize("   "), None);
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn caps_the_length() {
        let long = "x".repeat(150);
        assert_eq!(normalize(&long).unwrap().chars().count(), MAX_LABEL_CHARS);
        let unicode = "é".repeat(101);
        assert_eq!(normalize(&unicode).unwrap().chars().count(), 100);
    }
}
