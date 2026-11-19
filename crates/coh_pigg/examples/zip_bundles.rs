//! Zips every `.pigg` in a folder whole, as the web page zips a folder: `<out>/<bundle>.zip`, each
//! file under its path in the bundle. `tools/pigg-parity.py --zips` opens them with Python's
//! zipfile and with `unzip -t`, and checks every file against the Python reader.
//!
//!   cargo run -q --release -p coh_pigg --example zip_bundles -- <folder> <out>

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use coh_pigg::zip::{DosTime, Zip};
use coh_pigg::{parse_index, PiggEntry, PiggError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(folder), Some(out)) = (args.next(), args.next()) else {
        return Err("usage: zip_bundles <folder> <out>".into());
    };
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(&folder)?
        .map(|item| item.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    bundles.retain(|path| path.extension().is_some_and(|ext| ext == "pigg"));
    bundles.sort();
    for bundle in &bundles {
        let stem = bundle.file_stem().unwrap_or_default().to_string_lossy();
        let zip = zip_bundle(bundle)?;
        std::fs::write(Path::new(&out).join(format!("{stem}.zip")), zip)?;
    }
    Ok(())
}

fn zip_bundle(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut prefix = Vec::new();
    file.read_to_end(&mut prefix)?;
    let entries: Vec<PiggEntry> = match parse_index(&prefix) {
        Err(PiggError::NeedMoreBytes { .. }) => return Err("bundle cut short".into()),
        other => other?,
    };
    let stored_total: u64 = entries
        .iter()
        .map(|entry| entry.stored_range().end - entry.stored_range().start)
        .sum();
    let mut zip = Zip::new(stored_total as usize, DosTime::from_unix(0));
    for entry in &entries {
        let range = entry.stored_range();
        let mut stored = vec![0; (range.end - range.start) as usize];
        file.seek(SeekFrom::Start(range.start))?;
        file.read_exact(&mut stored)?;
        zip.add_entry(&entry.path, entry, &stored)?;
    }
    Ok(zip.finish())
}
