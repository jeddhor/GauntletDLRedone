//! NATIVE ENHANCEMENT (M19): texture replacements, not part of the original
//! game. A game texture is named by the first 16 hex digits of SHA-256
//! over its width and height (u16 little-endian) and its decoded level-0
//! RGBA as the client holds it (GS alpha: 0x80 opaque). GDL_TEXTURE_DUMP=DIR
//! writes each texture the client decodes as DIR/<name>.png (alpha doubled
//! to 0..255) and a line `<name>\t<label>\t<width>x<height>\t<flags>` in
//! DIR/index.tsv (the TEXDEF label and the record's flags, 0x4/0x8 clamp
//! U/V: research tooling); a texture pack (Settings::texture_pack) replaces a texture
//! with PACK/<name>.png of any size (alpha halved back, one mip level).
//! The generated HD pack (hd::generate, the same names and format) comes
//! after the user's pack: their replacements win. Lightmaps are not
//! replaced. Without a pack or a dump, nothing is hashed.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

struct Config {
    pack: Option<PathBuf>,
    generated: Option<PathBuf>,
    dump: Option<PathBuf>,
}

static CONFIG: RwLock<Config> = RwLock::new(Config { pack: None, generated: None, dump: None });

/// At startup: the user's pack directory, and the dump directory from
/// GDL_TEXTURE_DUMP.
pub fn configure(pack: Option<PathBuf>) {
    let dump = std::env::var_os("GDL_TEXTURE_DUMP").map(PathBuf::from);
    if let Some(d) = &dump {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut c) = CONFIG.write() {
        c.pack = pack.filter(|p| p.is_dir());
        c.dump = dump;
    }
}

/// The generated HD pack in use (None: off); textures decoded from then on
/// (the next level's) take it.
pub fn set_generated(dir: Option<PathBuf>) {
    if let Ok(mut c) = CONFIG.write() {
        c.generated = dir.filter(|p| p.is_dir());
    }
}

/// Whether a pack or a dump is configured.
pub fn active() -> bool {
    CONFIG.read().is_ok_and(|c| c.pack.is_some() || c.generated.is_some() || c.dump.is_some())
}

/// Whether a texture dump is configured (replace writes its files).
pub fn dumping() -> bool {
    CONFIG.read().is_ok_and(|c| c.dump.is_some())
}

/// A texture's file name stem.
pub fn name(width: u16, height: u16, rgba: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(width.to_le_bytes());
    h.update(height.to_le_bytes());
    h.update(rgba);
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The replacement (width, height, RGBA with GS alpha) for a decoded
/// texture, after writing it (with its TEXDEF `label` and record `flags`)
/// to the dump directory when one is set.
pub fn replace(width: u16, height: u16, rgba: &[u8], label: &str, flags: u16) -> Option<(u32, u32, Vec<u8>)> {
    let cfg = CONFIG.read().ok()?;
    if cfg.pack.is_none() && cfg.generated.is_none() && cfg.dump.is_none() {
        return None;
    }
    let stem = name(width, height, rgba);
    if let Some(dir) = &cfg.dump {
        let path = dir.join(format!("{stem}.png"));
        if !path.exists() && write_png(&path, width.into(), height.into(), rgba).is_ok() {
            use std::io::Write;
            let index = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("index.tsv"));
            let _ = index.and_then(|mut f| writeln!(f, "{stem}\t{label}\t{width}x{height}\t{flags:#x}"));
        }
    }
    [&cfg.pack, &cfg.generated].into_iter().flatten().find_map(|dir| read_png(&dir.join(format!("{stem}.png"))))
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let data: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], (u16::from(p[3]) * 2).min(255) as u8]).collect();
    let tmp = path.with_extension("png.tmp");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&tmp)?), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// A PNG as RGBA with GS alpha (8-bit, any colour type; at most 4096²).
fn read_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.width == 0 || info.height == 0 || 4096 < info.width || 4096 < info.height {
        return None;
    }
    let px = (info.width * info.height) as usize;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    let gs = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], ((u16::from(p[3]) * 128 + 127) / 255) as u8]).collect();
    Some((info.width, info.height, gs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_keeps_gs_alpha() {
        let dir = std::env::temp_dir().join(format!("gdl-texpack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rgba = vec![10, 20, 30, 0x80, 40, 50, 60, 0, 70, 80, 90, 0x40, 1, 2, 3, 0x7f];
        let path = dir.join(format!("{}.png", name(2, 2, &rgba)));
        write_png(&path, 2, 2, &rgba).unwrap();
        let (w, h, back) = read_png(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((w, h, back), (2, 2, rgba));
    }

    #[test]
    fn names_depend_on_size_and_texels() {
        let a = name(2, 2, &[0; 16]);
        assert_eq!(a.len(), 16);
        assert_ne!(a, name(4, 1, &[0; 16]));
        assert_ne!(a, name(2, 2, &[1; 16]));
    }
}
