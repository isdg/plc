//! Shared primitives for palace tools: wikilink parsing and
//! target normalization. Used by palace-orphans and (later) the
//! palace LSP for go-to-definition and rename across [[links]].

pub mod orphans;

use std::collections::HashSet;

/// ASCII-lowercase a string. Non-ASCII bytes pass through unchanged,
/// matching the byte-level behavior of the original zig implementation.
pub fn ascii_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Reduce a wikilink inner payload to its bare basename, lowercased.
///
/// Handles every shape we see in palace notes:
///   `target`              -> "target"
///   `target|alias`        -> "target"
///   `path/to/target`      -> "target"
///   `target#heading`      -> "target"
///   `target^block`        -> "target"
pub fn normalize_target(inner: &str) -> String {
    let end = inner
        .find(|c: char| c == '|' || c == '#' || c == '^')
        .unwrap_or(inner.len());
    let mut seg = &inner[..end];
    if let Some(s) = seg.rfind('/') {
        seg = &seg[s + 1..];
    }
    ascii_lower(seg.trim())
}

/// Scan markdown content for `[[...]]` outbound links. Each normalized
/// target is inserted into `targets`. Returns true iff any well-formed
/// `[[...]]` was seen.
///
/// The scanner operates on bytes since `[` and `]` are ASCII and cannot
/// appear inside a multi-byte UTF-8 sequence, so byte offsets are
/// always char-boundary safe for slicing the string.
pub fn scan_content(
    content: &str,
    targets: &mut HashSet<String>,
) -> bool {
    let bytes = content.as_bytes();
    let mut has_outbound = false;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j + 1] == b']' {
                has_outbound = true;
                let inner = &content[start..j];
                targets.insert(normalize_target(inner));
                i = j + 2;
            } else {
                i = start;
            }
        } else {
            i += 1;
        }
    }
    has_outbound
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_plain() {
        assert_eq!(normalize_target("Foo"), "foo");
    }

    #[test]
    fn normalize_strips_alias() {
        assert_eq!(normalize_target("Foo|bar"), "foo");
    }

    #[test]
    fn normalize_strips_heading() {
        assert_eq!(normalize_target("Foo#heading"), "foo");
    }

    #[test]
    fn normalize_strips_block() {
        assert_eq!(normalize_target("Foo^block"), "foo");
    }

    #[test]
    fn normalize_takes_basename() {
        assert_eq!(normalize_target("path/to/Foo"), "foo");
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_target("  Foo  "), "foo");
    }

    #[test]
    fn normalize_preserves_non_ascii() {
        assert_eq!(normalize_target("Привет"), "Привет");
    }

    #[test]
    fn scan_finds_outbound() {
        let mut t = HashSet::new();
        let has = scan_content(
            "see [[Bar]] and [[Baz|q]] also [[a/Qux#x]]",
            &mut t,
        );
        assert!(has);
        assert!(t.contains("bar"));
        assert!(t.contains("baz"));
        assert!(t.contains("qux"));
    }

    #[test]
    fn scan_no_outbound() {
        let mut t = HashSet::new();
        let has = scan_content("plain text [single] ]", &mut t);
        assert!(!has);
        assert!(t.is_empty());
    }

    #[test]
    fn scan_unterminated_link_falls_through() {
        // Scanner does not backtrack on an unterminated `[[`. After
        // hitting end-of-buffer without finding `]]`, it advances
        // past the opening brackets and continues — but if a later
        // valid `[[…]]` follows on the SAME line without a preceding
        // unmatched `]`, the unterminated prefix is absorbed into
        // its inner text. This matches the original zig behavior;
        // the test pins it so future refactors don't drift silently.
        let mut t = HashSet::new();
        let has = scan_content("[[broken-tail-no-close", &mut t);
        assert!(!has);
        assert!(t.is_empty());
    }
}
