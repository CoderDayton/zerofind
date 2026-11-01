//! The unit workers hand to the writer.

use std::path::Path;

/// Paths up to this many bytes travel inside the channel slot itself.
pub const INLINE: usize = 254;

/// One output record, already in display form.
///
/// A worker-allocated buffer freed by the writer thread costs a glibc arena lock handoff per
/// match: on 100k matches that was ~100k context switches and 4x the run time. Inline records
/// live in the bounded channel's preallocated slots, so the common case never allocates.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // the inline variant's size is the point
pub enum Record {
    // u8 len keeps tag + len + bytes at 256; a u16 pads the enum to 264.
    Inline { len: u8, bytes: [u8; INLINE] },
    Heap(Vec<u8>),
}

impl Record {
    pub fn new(path: &Path) -> Self {
        let src = path.as_os_str().as_encoded_bytes();
        if src.len() <= INLINE {
            let mut bytes = [0; INLINE];
            bytes[..src.len()].copy_from_slice(src);
            Self::Inline {
                len: src.len() as u8,
                bytes,
            }
        } else {
            Self::Heap(src.to_vec())
        }
    }

    #[cfg(test)]
    fn is_inline(&self) -> bool {
        matches!(self, Self::Inline { .. })
    }
}

impl AsRef<[u8]> for Record {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Inline { len, bytes } => &bytes[..*len as usize],
            Self::Heap(v) => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths_travel_inline() {
        let p = Path::new("src/domain/query.rs");
        let r = Record::new(p);
        assert!(r.is_inline());
        assert_eq!(r.as_ref(), b"src/domain/query.rs");
    }

    #[test]
    fn long_paths_round_trip_on_the_heap() {
        let exact = "a".repeat(INLINE);
        assert!(Record::new(Path::new(&exact)).is_inline());
        let long = "b/".repeat(INLINE);
        let r = Record::new(Path::new(&long));
        assert!(!r.is_inline());
        assert_eq!(r.as_ref(), long.as_bytes());
    }

    #[test]
    fn channel_slot_is_256_bytes() {
        assert_eq!(std::mem::size_of::<Record>(), 256);
    }
}
