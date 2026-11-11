//! Byte offsets ↔ LSP positions. A position's `character` counts code
//! units of the negotiated encoding, which for Cyrillic text differs from
//! the byte offsets the scanner works in.

use tower_lsp::lsp_types::{Position, PositionEncodingKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    /// UTF-8 when the client offers it, else the protocol default.
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Self {
        match offered {
            Some(kinds) if kinds.contains(&PositionEncodingKind::UTF8) => {
                Encoding::Utf8
            }
            _ => Encoding::Utf16,
        }
    }

    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }
    }

    /// Code units of `s` in this encoding.
    pub fn len(self, s: &str) -> u32 {
        match self {
            Encoding::Utf8 => s.len() as u32,
            Encoding::Utf16 => s.encode_utf16().count() as u32,
        }
    }
}

pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    enc: Encoding,
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str, enc: Encoding) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { text, starts, enc }
    }

    /// The text of line `n` without its line ending; empty past the end.
    pub fn line(&self, n: usize) -> &'a str {
        let Some(&start) = self.starts.get(n) else {
            return "";
        };
        let end = self.starts.get(n + 1).map_or(self.text.len(), |e| e - 1);
        let line = &self.text[start..end];
        line.strip_suffix('\r').unwrap_or(line)
    }

    /// The position of byte `offset`, which must sit on a char boundary.
    pub fn position(&self, offset: usize) -> Position {
        let n = self.starts.partition_point(|&s| s <= offset) - 1;
        let start = self.starts[n];
        let col = self.enc.len(&self.text[start..offset]);
        Position::new(n as u32, col)
    }

    /// The byte offset of `pos`. A position past the end of its line
    /// clamps to the line's end, and past the last line to the text's end,
    /// as the protocol asks.
    pub fn offset(&self, pos: Position) -> usize {
        let n = pos.line as usize;
        let Some(&start) = self.starts.get(n) else {
            return self.text.len();
        };
        let line = self.line(n);
        let mut units = 0;
        for (i, ch) in line.char_indices() {
            if units >= pos.character {
                return start + i;
            }
            units += match self.enc {
                Encoding::Utf8 => ch.len_utf8() as u32,
                Encoding::Utf16 => ch.len_utf16() as u32,
            };
        }
        start + line.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "ab\nдар [[x]]\r\n𝄞z";

    #[test]
    fn negotiates_utf8_only_when_offered() {
        let both = [PositionEncodingKind::UTF16, PositionEncodingKind::UTF8];
        assert_eq!(Encoding::negotiate(Some(&both)), Encoding::Utf8);
        let utf16 = [PositionEncodingKind::UTF16];
        assert_eq!(Encoding::negotiate(Some(&utf16)), Encoding::Utf16);
        assert_eq!(Encoding::negotiate(None), Encoding::Utf16);
    }

    #[test]
    fn utf16_counts_code_units() {
        let idx = LineIndex::new(TEXT, Encoding::Utf16);
        let link = TEXT.find("[[").unwrap();
        assert_eq!(idx.position(link), Position::new(1, 4));
        assert_eq!(idx.offset(Position::new(1, 4)), link);
        let z = TEXT.find('z').unwrap();
        assert_eq!(idx.position(z), Position::new(2, 2)); // 𝄞 is a pair
        assert_eq!(idx.offset(Position::new(2, 2)), z);
    }

    #[test]
    fn utf8_counts_bytes() {
        let idx = LineIndex::new(TEXT, Encoding::Utf8);
        let link = TEXT.find("[[").unwrap();
        assert_eq!(idx.position(link), Position::new(1, 7));
        assert_eq!(idx.offset(Position::new(1, 7)), link);
    }

    #[test]
    fn offsets_clamp_to_line_and_text_end() {
        let idx = LineIndex::new(TEXT, Encoding::Utf16);
        assert_eq!(idx.line(1), "дар [[x]]");
        let crlf = TEXT.find('\r').unwrap();
        assert_eq!(idx.offset(Position::new(1, 99)), crlf);
        assert_eq!(idx.offset(Position::new(9, 0)), TEXT.len());
    }
}
