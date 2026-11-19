//! One line per `.texture` in a folder, from inside its `.pigg` bundles and loose beside them:
//! `source<TAB>path<TAB>format<TAB>WxH<TAB>sha256 of the RGBA pixels`, or `jpeg<TAB>sha256` for a
//! JPEG, or `refused`. Decoded images go through PNG and back first, so the encoder is checked too.
//! `tools/pigg-parity.py --textures` diffs this against texture.py.
//!
//!   cargo run -q --release -p coh_pigg --example texture_hashes -- <folder>

use std::path::{Path, PathBuf};

use coh_pigg::texture::{decode_texture, TextureImage};
use coh_pigg::{extract_entry, parse_index};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::env::args()
        .nth(1)
        .ok_or("usage: texture_hashes <folder>")?;
    for bundle in files_ending(&folder, ".pigg")? {
        let bytes = std::fs::read(&bundle)?;
        let name = file_name(&bundle);
        for entry in parse_index(&bytes)? {
            if !entry.path.to_lowercase().ends_with(".texture") {
                continue;
            }
            let range = entry.stored_range();
            let file = extract_entry(&entry, &bytes[range.start as usize..range.end as usize])?;
            println!("{name}\t{}\t{}", entry.path, describe(&file)?);
        }
    }
    for loose in files_ending(&folder, ".texture")? {
        let name = file_name(&loose);
        println!("loose\t{name}\t{}", describe(&std::fs::read(&loose)?)?);
    }
    Ok(())
}

fn describe(file: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let Ok(texture) = decode_texture(file) else {
        return Ok("refused".to_string());
    };
    let (image, _) = texture.to_image_file()?;
    Ok(match texture.image {
        TextureImage::Jpeg(_) => format!("jpeg\t{}", sha256(&image)),
        TextureImage::Decoded {
            format,
            width,
            height,
            ..
        } => {
            let mut reader = png::Decoder::new(std::io::Cursor::new(image)).read_info()?;
            let mut pixels = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
            reader.next_frame(&mut pixels)?;
            format!("{}\t{width}x{height}\t{}", format.name(), sha256(&pixels))
        }
    })
}

fn files_ending(folder: &str, suffix: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)?
        .map(|item| item.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    found.retain(|path| path.to_string_lossy().ends_with(suffix));
    found.sort();
    Ok(found)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
