//! The user's disc: an image file (.iso, a raw .bin dump), an optical
//! drive's block device (read as an image) or a mounted disc's root
//! directory (a drive letter on Windows, a mount point on Linux). Paths
//! are the disc's ('/'-separated, any case, no ";1" version).
use gdl_formats::iso::{IsoEntry, IsoFs};
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

pub enum Source {
    Image(Box<IsoFs<BufReader<File>>>),
    Directory(PathBuf),
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        if path.is_dir() {
            return Ok(Self::Directory(path.to_path_buf()));
        }
        let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        Ok(Self::Image(Box::new(IsoFs::open(BufReader::with_capacity(1 << 20, file))?)))
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Image(_) => "image",
            Self::Directory(_) => "directory",
        }
    }

    /// The ISO volume label (images only).
    pub fn volume(&self) -> Option<String> {
        match self {
            Self::Image(fs) => Some(fs.volume_id().to_string()),
            Self::Directory(_) => None,
        }
    }

    /// The size of a disc file, None when absent.
    pub fn size(&self, path: &str) -> Option<u64> {
        match self {
            Self::Image(fs) => fs.find(path).map(|e| u64::from(e.size)),
            Self::Directory(root) => locate(root, path).and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()),
        }
    }

    /// Copy a disc file to `out`; `progress(bytes)` after each chunk
    /// (false cancels).
    pub fn copy(&mut self, path: &str, out: &mut impl Write, mut progress: impl FnMut(u64) -> bool) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Image(fs) => {
                let e: IsoEntry = fs.find(path).ok_or_else(|| format!("{path} is not on the disc"))?.clone();
                fs.copy(&e, out, progress)?;
            }
            Self::Directory(root) => {
                let p = locate(root, path).ok_or_else(|| format!("{path} is not on the disc"))?;
                let mut f = File::open(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
                let mut buf = vec![0u8; 1 << 20];
                let mut done = 0u64;
                loop {
                    let n = f.read(&mut buf).map_err(|e| format!("read error in {path}: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buf[..n])?;
                    done += n as u64;
                    if !progress(done) {
                        return Err("cancelled".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut v = Vec::with_capacity(self.size(path).unwrap_or(0) as usize);
        self.copy(path, &mut v, |_| true)?;
        Ok(v)
    }
}

/// A disc path under a mounted root, matched per component without case
/// and without a ";N" version (mounts show "SLUS_200.47", "slus_200.47"
/// or "SLUS_200.47;1" depending on their options).
fn locate(root: &Path, path: &str) -> Option<PathBuf> {
    let mut at = root.to_path_buf();
    for part in path.replace('\\', "/").split('/').filter(|s| !s.is_empty()) {
        let want = part.split(';').next()?.to_uppercase();
        let found = std::fs::read_dir(&at).ok()?.filter_map(Result::ok).find(|d| {
            let n = d.file_name().to_string_lossy().to_string();
            n.split(';').next().map(str::to_uppercase).as_deref() == Some(want.as_str())
        })?;
        at = found.path();
    }
    Some(at)
}
