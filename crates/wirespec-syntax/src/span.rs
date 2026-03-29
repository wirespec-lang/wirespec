/// A byte-offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Byte offset of the start.
    pub offset: u32,
    /// Byte length.
    pub len: u32,
}

impl Span {
    pub fn new(offset: u32, len: u32) -> Self {
        Self { offset, len }
    }

    pub fn end(&self) -> u32 {
        self.offset + self.len
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        let start = self.offset.min(other.offset);
        let end = self.end().max(other.end());
        Span {
            offset: start,
            len: end - start,
        }
    }
}
