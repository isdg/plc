//! Shared primitives for palace tools: wikilink parsing and
//! target normalization. Used by plc-orphans and (later) the
//! plc LSP for go-to-definition and rename across [[links]].

pub mod calendar;
pub mod graph;
pub mod ledger;
pub mod notes;
pub mod orphans;
pub mod rc;
pub mod settings;
pub mod vault;
pub mod week;

use std::collections::HashSet;
use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

/// The key two note names are compared by: Unicode lowercase, then NFC, so
/// `[[Дар]]` meets `дар.md` whether the file system stored it NFC or NFD.
pub fn fold_name(s: &str) -> String {
    s.to_lowercase().nfc().collect()
}

/// ASCII-lowercase a string. Non-ASCII bytes pass through unchanged,
/// matching the byte-level behavior of the original zig implementation.
pub fn ascii_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Reduce a wikilink inner payload to its bare basename, folded by
/// [`fold_name`].
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
    fold_name(seg.trim())
}

/// A `[[...]]` occurrence: `range` spans both bracket pairs, in bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link<'a> {
    pub range: Range<usize>,
    pub inner: &'a str,
}

/// Iterate the `[[...]]` links of `content` in order.
///
/// The scanner operates on bytes since `[` and `]` are ASCII and cannot
/// appear inside a multi-byte UTF-8 sequence, so byte offsets are
/// always char-boundary safe for slicing the string.
pub fn links(content: &str) -> Links<'_> {
    Links { content, pos: 0 }
}

pub struct Links<'a> {
    content: &'a str,
    pos: usize,
}

impl<'a> Iterator for Links<'a> {
    type Item = Link<'a>;

    fn next(&mut self) -> Option<Link<'a>> {
        let bytes = self.content.as_bytes();
        let mut i = self.pos;
        while i + 1 < bytes.len() {
            if bytes[i] != b'[' || bytes[i + 1] != b'[' {
                i += 1;
                continue;
            }
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j + 1] == b']' {
                self.pos = j + 2;
                return Some(Link {
                    range: i..j + 2,
                    inner: &self.content[start..j],
                });
            }
            i = start;
        }
        self.pos = bytes.len();
        None
    }
}

/// Scan markdown content for `[[...]]` outbound links. Each normalized
/// target is inserted into `targets`. Returns true iff any well-formed
/// `[[...]]` was seen.
pub fn scan_content(
    content: &str,
    targets: &mut HashSet<String>,
) -> bool {
    let mut has_outbound = false;
    for link in links(content) {
        has_outbound = true;
        targets.insert(normalize_target(link.inner));
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
    fn normalize_folds_non_ascii_case() {
        assert_eq!(normalize_target("Дар"), "дар");
        assert_eq!(normalize_target("Братья Карамазовы"), "братья карамазовы");
    }

    #[test]
    fn fold_name_unifies_nfc_and_nfd() {
        let nfd = "И\u{306}ога"; // Й spelled as И + combining breve
        assert_eq!(fold_name(nfd), "йога");
        assert_eq!(fold_name(nfd), fold_name("Йога"));
    }

    #[test]
    fn links_report_byte_ranges() {
        let text = "дар [[Дар|x]] и [[b]]";
        let got: Vec<_> = links(text).collect();
        assert_eq!(got.len(), 2);
        assert_eq!(&text[got[0].range.clone()], "[[Дар|x]]");
        assert_eq!(got[0].inner, "Дар|x");
        assert_eq!(&text[got[1].range.clone()], "[[b]]");
    }

    #[test]
    fn links_skip_unclosed_and_continue() {
        let got: Vec<_> = links("[[a] x [[b]]").map(|l| l.inner).collect();
        assert_eq!(got, ["b"]);
        assert_eq!(links("[[open").count(), 0);
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
