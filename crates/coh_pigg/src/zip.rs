//! Writes a zip of files taken out of bundles, in memory, for the page to hand to the browser.
//!
//! A bundle keeps most files as zlib streams, and a zlib stream is a zip's deflate data with two
//! bytes in front and four behind. So a compressed file goes in as the bundle stores it, never
//! squeezed again: the zip costs about the bundle's own size in memory, and no time compressing.
//! A file the bundle stores unpacked goes in unpacked. Every file is still unpacked once, to check
//! it and to work out the CRC a zip carries.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};

use crate::{extract_entry, PiggEntry, PiggError};

const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const END_SIGNATURE: u32 = 0x0605_4b50;
/// Version 2.0: deflate, no zip64.
const VERSION: u16 = 20;
/// Bit 11: names are UTF-8.
const UTF8_NAMES: u16 = 0x0800;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// Without zip64 a zip holds at most this many files, and 4 GB.
pub const MOST_FILES: usize = u16::MAX as usize;

#[derive(Debug, thiserror::Error)]
pub enum ZipError {
    #[error(transparent)]
    Pigg(#[from] PiggError),
    #[error("a zip can hold at most 65,535 files")]
    TooManyFiles,
    #[error("a zip can hold at most 4 GB")]
    TooBig,
}

/// A date and time as a zip records them: local time, to two seconds, from 1980 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DosTime {
    pub time: u16,
    pub date: u16,
}

impl DosTime {
    /// From seconds since 1970, already moved to the player's time zone: a zip holds clock time,
    /// not UTC. Before 1980 reads as 1980-01-01, the earliest a zip can say.
    pub fn from_unix(seconds: u64) -> DosTime {
        let days = (seconds / 86_400) as i64;
        let in_day = seconds % 86_400;
        // Days to a calendar date (Howard Hinnant's civil_from_days).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        if year < 1980 {
            return DosTime {
                time: 0,
                date: (1 << 5) | 1,
            };
        }
        let year = year.min(1980 + 127);
        DosTime {
            time: (((in_day / 3600) << 11) | ((in_day % 3600 / 60) << 5) | ((in_day % 60) / 2))
                as u16,
            date: (((year - 1980) << 9) | (month << 5) | day) as u16,
        }
    }
}

/// A zip being written. Add files, then [`Zip::finish`].
pub struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    files: usize,
    when: DosTime,
}

impl Zip {
    /// `capacity` is a guess at the finished size, to save the buffer regrowing.
    pub fn new(capacity: usize, when: DosTime) -> Zip {
        Zip {
            out: Vec::with_capacity(capacity),
            central: Vec::new(),
            files: 0,
            when,
        }
    }

    /// The zip's size so far, the closing directory included.
    pub fn len(&self) -> usize {
        self.out.len() + self.central.len() + 22
    }

    pub fn is_empty(&self) -> bool {
        self.files == 0
    }

    /// A file out of a bundle, unchanged, from its stored bytes (as [`extract_entry`] takes them).
    /// Returns the file's own bytes, unpacked and checked.
    pub fn add_entry(
        &mut self,
        name: &str,
        entry: &PiggEntry,
        stored: &[u8],
    ) -> Result<Vec<u8>, ZipError> {
        let file = extract_entry(entry, stored)?;
        let crc = crc_of(&file);
        match deflate_inside(stored, &file) {
            Some(deflate) if entry.is_compressed() => {
                self.push(name, DEFLATED, crc, deflate, file.len())?;
            }
            // The game stores a file unpacked when packing doesn't help (sounds), so neither
            // does trying again; and the zip's size is then known before it is made.
            _ if !entry.is_compressed() => self.push(name, STORED, crc, &file, file.len())?,
            _ => self.add_compressing(name, &file, crc)?,
        }
        Ok(file)
    }

    /// Bytes made here, such as a PNG. Already-packed kinds (PNG, JPEG) are stored as they are.
    pub fn add_file(&mut self, name: &str, file: &[u8]) -> Result<(), ZipError> {
        let crc = crc_of(file);
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".png") || lower.ends_with(".jpg") {
            return self.push(name, STORED, crc, file, file.len());
        }
        self.add_compressing(name, file, crc)
    }

    /// Deflated if that makes it smaller, stored otherwise.
    fn add_compressing(&mut self, name: &str, file: &[u8], crc: u32) -> Result<(), ZipError> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        let packed = encoder
            .write_all(file)
            .and_then(|()| encoder.finish())
            .ok()
            .filter(|packed| packed.len() < file.len());
        match packed {
            Some(packed) => self.push(name, DEFLATED, crc, &packed, file.len()),
            None => self.push(name, STORED, crc, file, file.len()),
        }
    }

    fn push(
        &mut self,
        name: &str,
        method: u16,
        crc: u32,
        data: &[u8],
        size: usize,
    ) -> Result<(), ZipError> {
        if self.files == MOST_FILES {
            return Err(ZipError::TooManyFiles);
        }
        let offset = u32::try_from(self.out.len()).map_err(|_| ZipError::TooBig)?;
        let packed = u32::try_from(data.len()).map_err(|_| ZipError::TooBig)?;
        let size = u32::try_from(size).map_err(|_| ZipError::TooBig)?;
        let name_len = u16::try_from(name.len()).map_err(|_| ZipError::TooBig)?;
        if self.len() + 30 + name.len() + data.len() + 46 + name.len() > u32::MAX as usize {
            return Err(ZipError::TooBig);
        }
        let shared = |out: &mut Vec<u8>| {
            put16(out, VERSION);
            put16(out, UTF8_NAMES);
            put16(out, method);
            put16(out, self.when.time);
            put16(out, self.when.date);
            put32(out, crc);
            put32(out, packed);
            put32(out, size);
            put16(out, name_len);
            put16(out, 0); // extra field
        };
        put32(&mut self.out, LOCAL_SIGNATURE);
        shared(&mut self.out);
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(data);

        put32(&mut self.central, CENTRAL_SIGNATURE);
        put16(&mut self.central, VERSION); // made by
        shared(&mut self.central);
        put16(&mut self.central, 0); // comment
        put16(&mut self.central, 0); // disk
        put16(&mut self.central, 0); // internal attributes
        put32(&mut self.central, 0); // external attributes
        put32(&mut self.central, offset);
        self.central.extend_from_slice(name.as_bytes());
        self.files += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        let directory_at = self.out.len() as u32;
        let directory_len = self.central.len() as u32;
        self.out.append(&mut self.central);
        put32(&mut self.out, END_SIGNATURE);
        put16(&mut self.out, 0); // this disk
        put16(&mut self.out, 0); // directory's disk
        put16(&mut self.out, self.files as u16);
        put16(&mut self.out, self.files as u16);
        put32(&mut self.out, directory_len);
        put32(&mut self.out, directory_at);
        put16(&mut self.out, 0); // comment
        self.out
    }
}

/// The deflate data inside a zlib stream, if the stream is plain: no preset dictionary, and its
/// checksum the last four bytes, so nothing trails the data.
fn deflate_inside<'a>(stored: &'a [u8], file: &[u8]) -> Option<&'a [u8]> {
    let [cmf, flg, ..] = *stored else { return None };
    let plain = stored.len() >= 6
        && cmf & 0x0f == 8
        && flg & 0x20 == 0
        && ((u16::from(cmf) << 8) | u16::from(flg)) % 31 == 0;
    let trailer = stored.get(stored.len().checked_sub(4)?..)?;
    (plain && trailer == adler32(file).to_be_bytes()).then(|| &stored[2..stored.len() - 4])
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the most bytes before `b` can overflow (RFC 1950's NMAX).
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

fn crc_of(bytes: &[u8]) -> u32 {
    let mut crc = Crc::new();
    crc.update(bytes);
    crc.sum()
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;
    use flate2::write::ZlibEncoder;
    use std::io::Read;

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn half(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    /// Every file in a zip, read through its central directory: (name, method, bytes).
    fn unzip(zip: &[u8]) -> Vec<(String, u16, Vec<u8>)> {
        let end = zip.len() - 22;
        assert_eq!(word(zip, end), END_SIGNATURE);
        let count = half(zip, end + 10) as usize;
        let mut at = word(zip, end + 16) as usize;
        let mut files = Vec::new();
        for _ in 0..count {
            assert_eq!(word(zip, at), CENTRAL_SIGNATURE);
            let method = half(zip, at + 10);
            let crc = word(zip, at + 16);
            let (packed, size) = (word(zip, at + 20) as usize, word(zip, at + 24) as usize);
            let name_len = half(zip, at + 28) as usize;
            let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
            let local = word(zip, at + 42) as usize;
            assert_eq!(word(zip, local), LOCAL_SIGNATURE);
            assert_eq!(&zip[local + 30..local + 30 + name_len], name.as_bytes());
            let data = &zip[local + 30 + name_len..local + 30 + name_len + packed];
            let file = match method {
                STORED => data.to_vec(),
                DEFLATED => {
                    let mut file = Vec::new();
                    DeflateDecoder::new(data).read_to_end(&mut file).unwrap();
                    file
                }
                other => panic!("method {other}"),
            };
            assert_eq!((file.len(), crc_of(&file)), (size, crc), "{name}");
            files.push((name, method, file));
            at += 46 + name_len;
        }
        files
    }

    fn zlib(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn entry(path: &str, file: &[u8], stored: &[u8], compressed: bool) -> PiggEntry {
        PiggEntry {
            path: path.to_string(),
            offset: 0,
            uncompressed_size: file.len() as u32,
            compressed_size: if compressed { stored.len() as u32 } else { 0 },
        }
    }

    #[test]
    fn bundle_files_go_in_as_stored_and_come_out_whole() {
        let text = b"Power Blast deals energy damage. ".repeat(200);
        let packed = zlib(&text);
        // The same stream with junk after its checksum: unpacks fine, but can't be copied as is.
        let mut trailing = packed.clone();
        trailing.extend_from_slice(b"junk");
        let raw = b"OggS raw sound".to_vec();

        let mut zip = Zip::new(0, DosTime::from_unix(0));
        assert!(zip.is_empty());
        let out = zip
            .add_entry(
                "texts/blast.txt",
                &entry("texts/blast.txt", &text, &packed, true),
                &packed,
            )
            .unwrap();
        assert_eq!(out, text);
        zip.add_entry(
            "texts/again.txt",
            &entry("texts/again.txt", &text, &trailing, true),
            &trailing,
        )
        .unwrap();
        zip.add_entry(
            "sound/a.ogg",
            &entry("sound/a.ogg", &raw, &raw, false),
            &raw,
        )
        .unwrap();
        zip.add_file("icons/a.png", b"\x89PNG not really").unwrap();
        let before = zip.len();
        let zip = zip.finish();
        assert_eq!(zip.len(), before);

        let files = unzip(&zip);
        let names: Vec<_> = files
            .iter()
            .map(|(name, method, _)| (name.as_str(), *method))
            .collect();
        assert_eq!(
            names,
            [
                ("texts/blast.txt", DEFLATED),
                ("texts/again.txt", DEFLATED),
                ("sound/a.ogg", STORED),
                ("icons/a.png", STORED),
            ]
        );
        assert_eq!(files[0].2, text);
        assert_eq!(files[1].2, text);
        assert_eq!(files[2].2, raw);
        // Copied, not squeezed again: the bundle's deflate bytes sit in the zip unchanged.
        let inner = &packed[2..packed.len() - 4];
        assert!(zip.windows(inner.len()).any(|window| window == inner));
    }

    #[test]
    fn a_damaged_file_is_refused_not_zipped() {
        let text = b"abc".repeat(100);
        let mut packed = zlib(&text);
        packed[4] ^= 0xff;
        let mut zip = Zip::new(0, DosTime::from_unix(0));
        let damaged = entry("a.txt", &text, &packed, true);
        assert!(matches!(
            zip.add_entry("a.txt", &damaged, &packed),
            Err(ZipError::Pigg(_))
        ));
        assert!(zip.is_empty());
    }

    #[test]
    fn dates_read_as_a_zip_writes_them() {
        // 2026-11-19 22:51:07 UTC.
        let when = DosTime::from_unix(1_795_128_667);
        assert_eq!(when.date, ((2026 - 1980) << 9) | (11 << 5) | 19);
        assert_eq!(when.time, (22 << 11) | (51 << 5) | 3);
        // 2000-02-29, a leap day.
        assert_eq!(
            DosTime::from_unix(951_782_400).date,
            (20 << 9) | (2 << 5) | 29
        );
        assert_eq!(DosTime::from_unix(0).date, (1 << 5) | 1);
    }
}
