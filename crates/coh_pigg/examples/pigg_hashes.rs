//! One line per entry of every `.pigg` in a folder: `bundle<TAB>index<TAB>path<TAB>size<TAB>sha256`.
//! `tools/pigg-parity.py` diffs this against the Python reader. Reads each table of contents the
//! way the web page will: a small first read, then whatever `parse_index` asks for.
//!
//!   cargo run -q --release -p coh_pigg --example pigg_hashes -- <folder>

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use coh_pigg::{extract_entry, parse_index, PiggError};
use sha2::{Digest, Sha256};

const FIRST_READ: usize = 4096;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::env::args()
        .nth(1)
        .ok_or("usage: pigg_hashes <folder>")?;
    let mut bundles: Vec<_> = std::fs::read_dir(&folder)?
        .map(|item| item.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    bundles.retain(|path| path.extension().is_some_and(|ext| ext == "pigg"));
    bundles.sort();
    for bundle in &bundles {
        hash_bundle(bundle)?;
    }
    Ok(())
}

fn hash_bundle(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let entries = loop_until_indexed(&mut file)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for (index, entry) in entries.iter().enumerate() {
        let range = entry.stored_range();
        let mut stored = vec![0; (range.end - range.start) as usize];
        file.seek(SeekFrom::Start(range.start))?;
        file.read_exact(&mut stored)?;
        let bytes = extract_entry(entry, &stored).map_err(|error| format!("{name}: {error}"))?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        println!("{name}\t{index}\t{}\t{}\t{digest}", entry.path, bytes.len());
    }
    Ok(())
}

fn loop_until_indexed(
    file: &mut File,
) -> Result<Vec<coh_pigg::PiggEntry>, Box<dyn std::error::Error>> {
    let mut want = FIRST_READ;
    loop {
        let mut prefix = Vec::with_capacity(want);
        file.seek(SeekFrom::Start(0))?;
        file.take(want as u64).read_to_end(&mut prefix)?;
        match parse_index(&prefix) {
            Err(PiggError::NeedMoreBytes { needed })
                if needed > prefix.len() && prefix.len() == want =>
            {
                want = needed;
            }
            other => return Ok(other?),
        }
    }
}
