//! Turns the game's `.texture` files into pictures. Ported from
//! `tools/pigg-wrangler/pigg_wrangler/texture.py`; `tools/pigg-parity.py --textures` checks every
//! real texture and a generated set covering each format against it, pixel for pixel.
//!
//! A `.texture` is a small header and then either a DDS image or a JPEG. JPEGs are handed back
//! untouched, since a browser shows them as they are. DDS images are decoded to RGBA.

use crate::ascii_lossy;

const TEXTURE_HEADER_LEN: usize = 32;
const DDS_HEADER_LEN: usize = 128;
const PF_FOURCC: u32 = 0x4;
const PF_RGB: u32 = 0x40;
const PF_LUMINANCE: u32 = 0x20000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TextureError {
    #[error("too short to be a .texture ({len} bytes)")]
    TooShort { len: usize },
    #[error("the image data offset {offset} is past the end of the file ({len} bytes)")]
    OffsetPastEnd { offset: usize, len: usize },
    #[error("the image is neither DDS nor JPEG (starts {magic:02x?})")]
    UnknownImage { magic: Vec<u8> },
    #[error("the DDS header is cut short")]
    ShortDdsHeader,
    #[error("this image format is not supported: {0}")]
    Unsupported(String),
    #[error("the image is {width}x{height}, which has no pixels")]
    NoPixels { width: u32, height: u32 },
    #[error(
        "a {width}x{height} {format} image needs {needed} bytes of pixels, the file has {found}"
    )]
    ShortPixels {
        format: &'static str,
        width: u32,
        height: u32,
        needed: usize,
        found: usize,
    },
    #[error("PNG encoding failed: {0}")]
    Png(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Dxt1,
    /// A DXT1 by another FourCC; decoded the same way.
    Nvt3,
    Dxt3,
    Dxt5,
    Bgra,
    Bgrx,
    Rgba,
    Rgbx,
    Bgr,
    Rgb,
    L8,
}

impl PixelFormat {
    /// The name the Python tool and DDS tools use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Dxt1 => "DXT1",
            Self::Nvt3 => "NVT3",
            Self::Dxt3 => "DXT3",
            Self::Dxt5 => "DXT5",
            Self::Bgra => "BGRA",
            Self::Bgrx => "BGRX",
            Self::Rgba => "RGBA",
            Self::Rgbx => "RGBX",
            Self::Bgr => "BGR",
            Self::Rgb => "RGB",
            Self::L8 => "L8",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureImage {
    Jpeg(Vec<u8>),
    Decoded {
        format: PixelFormat,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// The file name the texture was made from, e.g. `texture_library/gui/foo.dds`.
    pub original_name: String,
    pub image: TextureImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

impl Texture {
    /// Width and height of the picture itself. For a JPEG this is read from the JPEG, not the
    /// texture header, which can disagree (Homecoming's events_herocon says 1024×768 of a
    /// 1024×1024 picture). `None` for a JPEG with no size marker.
    pub fn size(&self) -> Option<(u32, u32)> {
        match &self.image {
            TextureImage::Jpeg(bytes) => jpeg_size(bytes),
            TextureImage::Decoded { width, height, .. } => Some((*width, *height)),
        }
    }

    /// `DXT5`, `BGRA`… or `JPEG`.
    pub fn format_name(&self) -> &'static str {
        match &self.image {
            TextureImage::Jpeg(_) => "JPEG",
            TextureImage::Decoded { format, .. } => format.name(),
        }
    }

    /// The picture as a file a browser or image viewer opens: PNG, or the JPEG as stored.
    pub fn to_image_file(&self) -> Result<(Vec<u8>, ImageKind), TextureError> {
        match &self.image {
            TextureImage::Jpeg(bytes) => Ok((bytes.clone(), ImageKind::Jpeg)),
            TextureImage::Decoded {
                width,
                height,
                rgba,
                ..
            } => Ok((encode_png(*width, *height, rgba)?, ImageKind::Png)),
        }
    }
}

pub fn decode_texture(data: &[u8]) -> Result<Texture, TextureError> {
    if data.len() < TEXTURE_HEADER_LEN {
        return Err(TextureError::TooShort { len: data.len() });
    }
    let offset = u32_at(data, 0) as usize;
    let payload = data
        .get(offset..)
        .filter(|payload| payload.len() >= 4)
        .ok_or(TextureError::OffsetPastEnd {
            offset,
            len: data.len(),
        })?;
    let original_name = original_name(&data[..offset]);

    let image = if payload.starts_with(b"DDS ") {
        decode_dds(payload)?
    } else if payload.starts_with(&[0xFF, 0xD8]) {
        TextureImage::Jpeg(payload.to_vec())
    } else {
        return Err(TextureError::UnknownImage {
            magic: payload[..4].to_vec(),
        });
    };
    Ok(Texture {
        original_name,
        image,
    })
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, TextureError> {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let png_error = |error: png::EncodingError| TextureError::Png(error.to_string());
    let mut writer = encoder.write_header().map_err(png_error)?;
    writer.write_image_data(rgba).map_err(png_error)?;
    writer.finish().map_err(png_error)?;
    Ok(file)
}

/// A JPEG's size, from its first start-of-frame marker.
fn jpeg_size(jpeg: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 4 <= jpeg.len() {
        if jpeg[at] != 0xFF {
            return None;
        }
        let marker = jpeg[at + 1];
        // Fill bytes, and markers that carry no length.
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            at += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        // Every SOFn but DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let frame = jpeg.get(at + 5..at + 9)?;
            let height = u16::from_be_bytes([frame[0], frame[1]]);
            let width = u16::from_be_bytes([frame[2], frame[3]]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + length;
    }
    None
}

/// The name sits after a `TX2` marker in the header, up to a NUL.
fn original_name(header: &[u8]) -> String {
    let Some(marker) = header.windows(3).position(|window| window == b"TX2") else {
        return String::new();
    };
    let name = &header[marker + 3..];
    let end = name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(name.len());
    ascii_lossy(&name[..end])
}

fn decode_dds(dds: &[u8]) -> Result<TextureImage, TextureError> {
    if dds.len() < DDS_HEADER_LEN {
        return Err(TextureError::ShortDdsHeader);
    }
    let height = u32_at(dds, 12);
    let width = u32_at(dds, 16);
    if width == 0 || height == 0 {
        return Err(TextureError::NoPixels { width, height });
    }
    let format = pixel_format(dds)?;
    let pixels = &dds[DDS_HEADER_LEN..];
    let needed = pixel_bytes_needed(format, width, height);
    if pixels.len() < needed {
        return Err(TextureError::ShortPixels {
            format: format.name(),
            width,
            height,
            needed,
            found: pixels.len(),
        });
    }
    let rgba = match format {
        PixelFormat::Dxt1 | PixelFormat::Nvt3 => {
            decode_blocks(pixels, width, height, 8, dxt1_block)
        }
        PixelFormat::Dxt3 => decode_blocks(pixels, width, height, 16, dxt3_block),
        PixelFormat::Dxt5 => decode_blocks(pixels, width, height, 16, dxt5_block),
        PixelFormat::Bgra => swizzle(pixels, 4, |p| [p[2], p[1], p[0], p[3]]),
        PixelFormat::Bgrx => swizzle(pixels, 4, |p| [p[2], p[1], p[0], 255]),
        PixelFormat::Rgba => swizzle(pixels, 4, |p| [p[0], p[1], p[2], p[3]]),
        PixelFormat::Rgbx => swizzle(pixels, 4, |p| [p[0], p[1], p[2], 255]),
        PixelFormat::Bgr => swizzle(pixels, 3, |p| [p[2], p[1], p[0], 255]),
        PixelFormat::Rgb => swizzle(pixels, 3, |p| [p[0], p[1], p[2], 255]),
        PixelFormat::L8 => swizzle(pixels, 1, |p| [p[0], p[0], p[0], 255]),
    };
    let rgba = rgba[..width as usize * height as usize * 4].to_vec();
    Ok(TextureImage::Decoded {
        format,
        width,
        height,
        rgba,
    })
}

/// The same choices, in the same order, as `parse_dds_header` in texture.py.
fn pixel_format(dds: &[u8]) -> Result<PixelFormat, TextureError> {
    let flags = u32_at(dds, 80);
    let fourcc = &dds[84..88];
    let bit_count = u32_at(dds, 88);
    let (r_mask, b_mask, a_mask) = (u32_at(dds, 92), u32_at(dds, 100), u32_at(dds, 104));

    if flags & PF_FOURCC != 0 {
        return match fourcc {
            b"DXT1" => Ok(PixelFormat::Dxt1),
            b"NVT3" => Ok(PixelFormat::Nvt3),
            b"DXT3" => Ok(PixelFormat::Dxt3),
            b"DXT5" => Ok(PixelFormat::Dxt5),
            other => Err(TextureError::Unsupported(format!(
                "FourCC {}",
                String::from_utf8_lossy(other)
            ))),
        };
    }
    if flags & PF_RGB != 0 {
        return match (bit_count, r_mask, b_mask) {
            (32, 0x00FF_0000, 0x0000_00FF) if a_mask != 0 => Ok(PixelFormat::Bgra),
            (32, 0x00FF_0000, 0x0000_00FF) => Ok(PixelFormat::Bgrx),
            (32, 0x0000_00FF, 0x00FF_0000) if a_mask != 0 => Ok(PixelFormat::Rgba),
            (32, 0x0000_00FF, 0x00FF_0000) => Ok(PixelFormat::Rgbx),
            (24, 0x00FF_0000, _) => Ok(PixelFormat::Bgr),
            (24, _, _) => Ok(PixelFormat::Rgb),
            _ => Err(TextureError::Unsupported(format!(
                "{bit_count}-bit RGB with red mask {r_mask:#010x}"
            ))),
        };
    }
    if flags & PF_LUMINANCE != 0 {
        return match bit_count {
            8 => Ok(PixelFormat::L8),
            _ => Err(TextureError::Unsupported(format!(
                "{bit_count}-bit luminance"
            ))),
        };
    }
    Err(TextureError::Unsupported(format!(
        "pixel format flags {flags:#010x}"
    )))
}

fn pixel_bytes_needed(format: PixelFormat, width: u32, height: u32) -> usize {
    let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
    let pixels = width as usize * height as usize;
    match format {
        PixelFormat::Dxt1 | PixelFormat::Nvt3 => blocks * 8,
        PixelFormat::Dxt3 | PixelFormat::Dxt5 => blocks * 16,
        PixelFormat::Bgra | PixelFormat::Bgrx | PixelFormat::Rgba | PixelFormat::Rgbx => pixels * 4,
        PixelFormat::Bgr | PixelFormat::Rgb => pixels * 3,
        PixelFormat::L8 => pixels,
    }
}

fn swizzle(pixels: &[u8], stride: usize, to_rgba: impl Fn(&[u8]) -> [u8; 4]) -> Vec<u8> {
    pixels.chunks_exact(stride).flat_map(to_rgba).collect()
}

/// Lays out 4x4 blocks across the image, dropping the parts of edge blocks past its size.
fn decode_blocks(
    pixels: &[u8],
    width: u32,
    height: u32,
    block_len: usize,
    decode_block: fn(&[u8]) -> [[u8; 4]; 16],
) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let blocks_wide = width.div_ceil(4);
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in pixels
        .chunks_exact(block_len)
        .take(blocks_wide * height.div_ceil(4))
        .enumerate()
    {
        let (block_x, block_y) = (index % blocks_wide * 4, index / blocks_wide * 4);
        for (i, pixel) in decode_block(block).iter().enumerate() {
            let (x, y) = (block_x + i % 4, block_y + i / 4);
            if x < width && y < height {
                let at = (y * width + x) * 4;
                rgba[at..at + 4].copy_from_slice(pixel);
            }
        }
    }
    rgba
}

fn dxt1_block(block: &[u8]) -> [[u8; 4]; 16] {
    color_block(block, false)
}

/// Explicit alpha, four bits a pixel, then a colour block.
fn dxt3_block(block: &[u8]) -> [[u8; 4]; 16] {
    let alpha_bits = u64::from_le_bytes(block[..8].try_into().expect("eight bytes"));
    let mut pixels = color_block(&block[8..], true);
    for (i, pixel) in pixels.iter_mut().enumerate() {
        pixel[3] = ((alpha_bits >> (4 * i)) & 0xF) as u8 * 17;
    }
    pixels
}

/// Two alpha endpoints and three-bit indexes into a palette built from them, then a colour block.
fn dxt5_block(block: &[u8]) -> [[u8; 4]; 16] {
    let (a0, a1) = (u32::from(block[0]), u32::from(block[1]));
    let mut palette = [a0, a1, 0, 0, 0, 0, 0, 255];
    if a0 > a1 {
        for i in 1..7 {
            palette[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1 + 3) / 7;
        }
    } else {
        for i in 1..5 {
            palette[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1 + 2) / 5;
        }
    }
    let mut bits = [0u8; 8];
    bits[..6].copy_from_slice(&block[2..8]);
    let alpha_bits = u64::from_le_bytes(bits);
    let mut pixels = color_block(&block[8..], true);
    for (i, pixel) in pixels.iter_mut().enumerate() {
        pixel[3] = palette[((alpha_bits >> (3 * i)) & 0x7) as usize] as u8;
    }
    pixels
}

/// A DXT colour block. DXT1 switches to three colours and transparent black when `c0 <= c1`;
/// DXT3 and DXT5 never do (EXT_texture_compression_s3tc: "treated as though color0 > color1").
/// texture.py used to switch for DXT5 too; in Homecoming that miscoloured one weapon glow.
fn color_block(block: &[u8], four_color_only: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let lookup = u32::from_le_bytes(block[4..8].try_into().expect("four bytes"));
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |weight0: u32, weight1: u32, round: u32| {
        let total = weight0 + weight1;
        std::array::from_fn::<u8, 3, _>(|ch| {
            ((weight0 * u32::from(p0[ch]) + weight1 * u32::from(p1[ch]) + round) / total) as u8
        })
    };
    let opaque = |rgb: [u8; 3]| [rgb[0], rgb[1], rgb[2], 255];
    let palette = if c0 > c1 || four_color_only {
        [
            opaque(p0),
            opaque(p1),
            opaque(mix(2, 1, 1)),
            opaque(mix(1, 2, 1)),
        ]
    } else {
        [opaque(p0), opaque(p1), opaque(mix(1, 1, 1)), [0, 0, 0, 0]]
    };
    std::array::from_fn(|i| palette[((lookup >> (2 * i)) & 0x3) as usize])
}

fn rgb565(color: u16) -> [u8; 3] {
    let color = u32::from(color);
    [
        ((color >> 11 & 0x1F) * 255 / 31) as u8,
        ((color >> 5 & 0x3F) * 255 / 63) as u8,
        ((color & 0x1F) * 255 / 31) as u8,
    ]
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("caller checked the length"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.texture` around a DDS of the given format bits and pixel bytes.
    fn texture(
        flags: u32,
        fourcc: &[u8; 4],
        bits: u32,
        masks: [u32; 4],
        w: u32,
        h: u32,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut header = vec![0u8; 64];
        header[..4].copy_from_slice(&64u32.to_le_bytes());
        header[0x1D..0x20].copy_from_slice(b"TX2");
        header[0x20..0x29].copy_from_slice(b"a/b.dds\0\0");
        let mut dds = vec![0u8; DDS_HEADER_LEN];
        dds[..4].copy_from_slice(b"DDS ");
        dds[12..16].copy_from_slice(&h.to_le_bytes());
        dds[16..20].copy_from_slice(&w.to_le_bytes());
        dds[80..84].copy_from_slice(&flags.to_le_bytes());
        dds[84..88].copy_from_slice(fourcc);
        dds[88..92].copy_from_slice(&bits.to_le_bytes());
        for (n, mask) in masks.iter().enumerate() {
            dds[92 + 4 * n..96 + 4 * n].copy_from_slice(&mask.to_le_bytes());
        }
        [header, dds, pixels.to_vec()].concat()
    }

    /// The case texture.py got wrong: c0 < c1 in a DXT5 block still means four colours.
    #[test]
    fn dxt5_colour_blocks_always_use_four_colours() {
        let mut block = [0u8; 16];
        block[0] = 255; // both alpha endpoints opaque
        block[1] = 255;
        block[8..10].copy_from_slice(&0x0000u16.to_le_bytes()); // c0 black
        block[10..12].copy_from_slice(&0xFFFFu16.to_le_bytes()); // c1 white, so c0 < c1
        block[12..16].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // every pixel index 3
        let data = texture(PF_FOURCC, b"DXT5", 0, [0; 4], 4, 4, &block);
        let TextureImage::Decoded { rgba, .. } = decode_texture(&data).unwrap().image else {
            panic!("expected a decoded image");
        };
        // Index 3 is two thirds of the way from c0 to c1, not DXT1's transparent black.
        assert_eq!(&rgba[..4], &[170, 170, 170, 255]);
    }

    #[test]
    fn missing_pixels_are_refused_not_drawn_black() {
        let data = texture(
            PF_RGB,
            b"\0\0\0\0",
            32,
            [0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000],
            2,
            2,
            &[0; 15],
        );
        assert!(matches!(
            decode_texture(&data),
            Err(TextureError::ShortPixels {
                needed: 16,
                found: 15,
                ..
            })
        ));
    }

    #[test]
    fn a_jpeg_reports_its_own_size_not_the_headers() {
        // SOI, an APP0 segment to skip, then a baseline frame of 1024 wide by 768 high.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0xAA, 0xBB];
        jpeg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x03, 0x00, 0x04, 0x00, 0x03]);
        let mut file = vec![0u8; TEXTURE_HEADER_LEN];
        file[..4].copy_from_slice(&(TEXTURE_HEADER_LEN as u32).to_le_bytes());
        file[8..12].copy_from_slice(&64u32.to_le_bytes());
        file[12..16].copy_from_slice(&64u32.to_le_bytes());
        file.extend(&jpeg);
        let texture = decode_texture(&file).unwrap();
        assert_eq!(texture.format_name(), "JPEG");
        assert_eq!(texture.size(), Some((1024, 768)));
        assert_eq!(jpeg_size(&jpeg[..12]), None);
    }

    #[test]
    fn a_png_round_trips_to_the_same_pixels() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|n| n as u8 * 10).collect();
        let png = encode_png(3, 2, &rgba).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut decoded).unwrap();
        assert_eq!(decoded, rgba);
    }
}
