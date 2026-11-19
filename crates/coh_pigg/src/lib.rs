//! Reads City of Heroes `.pigg` bundles. Ported from `tools/pigg-wrangler/pigg_wrangler/pigg.py`,
//! which stays the reference: `tools/pigg-parity.py` checks every entry against it byte for byte.
//!
//! No file access here. The caller hands over bytes, so the web page can read only the slice it
//! needs from a 137 MB bundle instead of loading all of it.

use std::io::Read;
use std::ops::Range;

use flate2::read::ZlibDecoder;

const BUNDLE_SIGNATURE: u32 = 0x123;
const ENTRY_SIGNATURE: u32 = 0x3456;
const NAME_TABLE_SIGNATURE: u32 = 0x6789;
const HEADER_LEN: usize = 16;
const ENTRY_LEN: usize = 48;
const NAME_TABLE_HEADER_LEN: usize = 12;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PiggError {
    /// Not an error so much as a request: read this many bytes from the start and call again.
    #[error("the bundle's table of contents needs the first {needed} bytes")]
    NeedMoreBytes { needed: usize },
    #[error("not a .pigg bundle (signature {found:#x})")]
    NotABundle { found: u32 },
    #[error("entry {index} has signature {found:#x}, not {ENTRY_SIGNATURE:#x}")]
    BadEntry { index: usize, found: u32 },
    #[error("the filename table has signature {found:#x}, not {NAME_TABLE_SIGNATURE:#x}")]
    BadNameTable { found: u32 },
    #[error("the filename table lists {names} names for {entries} entries")]
    NameCountMismatch { names: usize, entries: usize },
    #[error("the filename table's names run past its stated length")]
    NameTableOverrun,
    #[error("entry {index} points at name {name_index}, but there are only {names}")]
    NameOutOfRange {
        index: usize,
        name_index: usize,
        names: usize,
    },
    #[error("{path}: handed {handed} stored bytes, the entry holds {expected}")]
    WrongStoredLength {
        path: String,
        handed: usize,
        expected: usize,
    },
    #[error("{path}: decompression failed: {reason}")]
    Decompress { path: String, reason: String },
    #[error("{path}: decompressed to {got} bytes, the entry says {expected}")]
    WrongSize {
        path: String,
        got: usize,
        expected: usize,
    },
}

/// One file inside a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiggEntry {
    /// The path inside the bundle, e.g. `bin/powers.bin`.
    pub path: String,
    pub offset: u32,
    pub uncompressed_size: u32,
    /// Zero means the file is stored as-is.
    pub compressed_size: u32,
}

impl PiggEntry {
    pub fn is_compressed(&self) -> bool {
        self.compressed_size > 0
    }

    /// Where this entry's stored bytes sit in the bundle; the slice to hand to [`extract_entry`].
    pub fn stored_range(&self) -> Range<u64> {
        let len = if self.is_compressed() {
            self.compressed_size
        } else {
            self.uncompressed_size
        };
        let start = u64::from(self.offset);
        start..start + u64::from(len)
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// Reads a bundle's table of contents from its first bytes.
///
/// Hand it any prefix of the file. If the prefix is too short it returns
/// [`PiggError::NeedMoreBytes`] with the exact length to read. Starting from at least the
/// 16-byte header, that takes at most two retries: one for the entries and the name table's
/// header, one for the names themselves.
pub fn parse_index(prefix: &[u8]) -> Result<Vec<PiggEntry>, PiggError> {
    need(prefix, HEADER_LEN)?;
    let signature = read_u32(prefix, 0)?;
    if signature != BUNDLE_SIGNATURE {
        return Err(PiggError::NotABundle { found: signature });
    }
    let entry_count = read_u32(prefix, 12)? as usize;
    let name_table_at = HEADER_LEN + ENTRY_LEN * entry_count;
    need(prefix, name_table_at + NAME_TABLE_HEADER_LEN)?;

    let table_signature = read_u32(prefix, name_table_at)?;
    if table_signature != NAME_TABLE_SIGNATURE {
        return Err(PiggError::BadNameTable {
            found: table_signature,
        });
    }
    let name_count = read_u32(prefix, name_table_at + 4)? as usize;
    if name_count != entry_count {
        return Err(PiggError::NameCountMismatch {
            names: name_count,
            entries: entry_count,
        });
    }
    let names_at = name_table_at + NAME_TABLE_HEADER_LEN;
    let names_end = names_at + read_u32(prefix, name_table_at + 8)? as usize;
    need(prefix, names_end)?;
    let names = read_names(&prefix[names_at..names_end], name_count)?;

    (0..entry_count)
        .map(|index| {
            let at = HEADER_LEN + ENTRY_LEN * index;
            let field = |n: usize| read_u32(prefix, at + 4 * n);
            let entry_signature = field(0)?;
            if entry_signature != ENTRY_SIGNATURE {
                return Err(PiggError::BadEntry {
                    index,
                    found: entry_signature,
                });
            }
            let name_index = field(1)? as usize;
            let path = names
                .get(name_index)
                .cloned()
                .ok_or(PiggError::NameOutOfRange {
                    index,
                    name_index,
                    names: names.len(),
                })?;
            Ok(PiggEntry {
                path,
                uncompressed_size: field(2)?,
                offset: field(4)?,
                compressed_size: field(11)?,
            })
        })
        .collect()
}

/// Turns an entry's stored bytes (the slice at [`PiggEntry::stored_range`]) into the file.
pub fn extract_entry(entry: &PiggEntry, stored: &[u8]) -> Result<Vec<u8>, PiggError> {
    let expected_stored = (entry.stored_range().end - entry.stored_range().start) as usize;
    if stored.len() != expected_stored {
        return Err(PiggError::WrongStoredLength {
            path: entry.path.clone(),
            handed: stored.len(),
            expected: expected_stored,
        });
    }
    if !entry.is_compressed() {
        return Ok(stored.to_vec());
    }
    let mut file = Vec::with_capacity(entry.uncompressed_size as usize);
    ZlibDecoder::new(stored)
        .read_to_end(&mut file)
        .map_err(|error| PiggError::Decompress {
            path: entry.path.clone(),
            reason: error.to_string(),
        })?;
    if file.len() != entry.uncompressed_size as usize {
        return Err(PiggError::WrongSize {
            path: entry.path.clone(),
            got: file.len(),
            expected: entry.uncompressed_size as usize,
        });
    }
    Ok(file)
}

fn need(prefix: &[u8], needed: usize) -> Result<(), PiggError> {
    if prefix.len() < needed {
        return Err(PiggError::NeedMoreBytes { needed });
    }
    Ok(())
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, PiggError> {
    let word = bytes
        .get(at..at + 4)
        .ok_or(PiggError::NeedMoreBytes { needed: at + 4 })?;
    Ok(u32::from_le_bytes(
        word.try_into().expect("slice is four bytes"),
    ))
}

/// Each name is a length, then that many bytes, NUL-padded.
fn read_names(table: &[u8], count: usize) -> Result<Vec<String>, PiggError> {
    let mut names = Vec::with_capacity(count);
    let mut at = 0;
    for _ in 0..count {
        let len = read_u32(table, at).map_err(|_| PiggError::NameTableOverrun)? as usize;
        let raw = table
            .get(at + 4..at + 4 + len)
            .ok_or(PiggError::NameTableOverrun)?;
        names.push(ascii_lossy(trim_trailing_nuls(raw)));
        at += 4 + len;
    }
    Ok(names)
}

fn trim_trailing_nuls(raw: &[u8]) -> &[u8] {
    let end = raw
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |last| last + 1);
    &raw[..end]
}

/// Python's `decode("ascii", errors="replace")`: each non-ASCII byte becomes U+FFFD.
fn ascii_lossy(raw: &[u8]) -> String {
    raw.iter()
        .map(|&byte| {
            if byte.is_ascii() {
                byte as char
            } else {
                '\u{FFFD}'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    /// A two-entry bundle: `a/stored.txt` kept as-is, `b/packed.txt` zlib-compressed.
    fn tiny_bundle() -> (Vec<u8>, &'static [u8], Vec<u8>) {
        let stored: &[u8] = b"plain bytes";
        let packed_plain = b"compressed bytes compressed bytes compressed bytes".to_vec();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&packed_plain).unwrap();
        let packed = encoder.finish().unwrap();

        let names: [&[u8]; 2] = [b"a/stored.txt\0", b"b/packed.txt\0"];
        let names_len: usize = names.iter().map(|name| 4 + name.len()).sum();
        let data_at = HEADER_LEN + 2 * ENTRY_LEN + NAME_TABLE_HEADER_LEN + names_len;

        let mut bundle = Vec::new();
        for word in [BUNDLE_SIGNATURE, 0x0002_0002, 0x0030_0010, 2] {
            bundle.extend_from_slice(&word.to_le_bytes());
        }
        let entry = |name_index: u32, size: usize, offset: usize, compressed: usize| {
            let mut fields = [0u32; 12];
            fields[0] = ENTRY_SIGNATURE;
            fields[1] = name_index;
            fields[2] = size as u32;
            fields[4] = offset as u32;
            fields[11] = compressed as u32;
            fields
                .iter()
                .flat_map(|field| field.to_le_bytes())
                .collect::<Vec<u8>>()
        };
        // Entries point at names out of order, as real bundles may.
        bundle.extend(entry(
            1,
            packed_plain.len(),
            data_at + stored.len(),
            packed.len(),
        ));
        bundle.extend(entry(0, stored.len(), data_at, 0));
        for word in [NAME_TABLE_SIGNATURE, 2, names_len as u32] {
            bundle.extend_from_slice(&word.to_le_bytes());
        }
        for name in names {
            bundle.extend_from_slice(&(name.len() as u32).to_le_bytes());
            bundle.extend_from_slice(name);
        }
        bundle.extend_from_slice(stored);
        bundle.extend_from_slice(&packed);
        (bundle, stored, packed_plain)
    }

    fn extract(bundle: &[u8], entry: &PiggEntry) -> Result<Vec<u8>, PiggError> {
        let range = entry.stored_range();
        extract_entry(entry, &bundle[range.start as usize..range.end as usize])
    }

    #[test]
    fn reads_stored_and_compressed_entries() {
        let (bundle, stored, packed_plain) = tiny_bundle();
        let entries = parse_index(&bundle).unwrap();
        assert_eq!(entries[0].path, "b/packed.txt");
        assert_eq!(entries[1].path, "a/stored.txt");
        assert_eq!(extract(&bundle, &entries[0]).unwrap(), packed_plain);
        assert_eq!(extract(&bundle, &entries[1]).unwrap(), stored);
    }

    /// The page relies on this: start small, read what it asks for, and it settles.
    #[test]
    fn a_short_prefix_says_how_much_to_read() {
        let (bundle, _, _) = tiny_bundle();
        let mut len = HEADER_LEN;
        for _ in 0..3 {
            match parse_index(&bundle[..len]) {
                Err(PiggError::NeedMoreBytes { needed }) => len = needed,
                Ok(entries) => return assert_eq!(entries.len(), 2),
                Err(other) => panic!("{other}"),
            }
        }
        panic!("still asking for more bytes after three reads");
    }

    #[test]
    fn a_corrupt_entry_is_refused_not_guessed() {
        let (mut bundle, _, _) = tiny_bundle();
        bundle[HEADER_LEN] ^= 0xFF;
        assert!(matches!(
            parse_index(&bundle),
            Err(PiggError::BadEntry { index: 0, .. })
        ));
    }

    #[test]
    fn a_short_decompression_is_refused() {
        let (bundle, _, _) = tiny_bundle();
        let mut entry = parse_index(&bundle).unwrap().remove(0);
        let real = extract(&bundle, &entry).unwrap();
        entry.uncompressed_size += 1;
        assert_eq!(
            extract(&bundle, &entry),
            Err(PiggError::WrongSize {
                path: entry.path.clone(),
                got: real.len(),
                expected: real.len() + 1
            })
        );
    }
}
