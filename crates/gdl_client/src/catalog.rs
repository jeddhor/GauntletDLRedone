//! Asset catalog: disc package labels → extracted files, resolved through
//! the name proposals (private/reports/wad-independent.json) and the
//! extraction manifest (private/wad/manifest.json), with the decoded SHA-256
//! checked on every read (the same identity rule as run_play.py's resolve).
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

pub struct Catalog {
    /// Upper-case label → (file, decoded sha256).
    entries: HashMap<String, (PathBuf, String)>,
}

impl Catalog {
    pub fn open(names: &Path, archive: &Path, root: &Path) -> Result<Self, Box<dyn Error>> {
        let names: serde_json::Value = serde_json::from_slice(&crate::bounded_read(names)?)?;
        let archive: serde_json::Value = serde_json::from_slice(&crate::bounded_read(archive)?)?;
        if names["source_sha256"] != archive["source_sha256"] {
            return Err("name candidates and extraction refer to different archives".into());
        }
        let files = archive["entries"].as_array().ok_or("manifest entries")?;
        let mut count: HashMap<String, usize> = HashMap::new();
        let mut entries = HashMap::new();
        for e in names["entries"].as_array().ok_or("name entries")? {
            let index = e["index"].as_u64().ok_or("entry index")? as usize;
            let sha = e["decoded_sha256"].as_str().ok_or("entry sha")?.to_string();
            let file = files.get(index).and_then(|f| f["output_file"].as_str()).ok_or("manifest output file")?;
            for n in e["proposed_names"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
                let n = n.to_uppercase();
                *count.entry(n.clone()).or_default() += 1;
                entries.insert(n, (root.join(file), sha.clone()));
            }
        }
        // Ambiguous labels are not resolvable.
        entries.retain(|n, _| count.get(n) == Some(&1));
        Ok(Self { entries })
    }

    /// Every resolvable label (upper case), sorted.
    pub fn labels(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    pub fn has(&self, label: &str) -> bool {
        self.entries.contains_key(&label.to_uppercase())
    }

    pub fn path(&self, label: &str) -> Result<&Path, Box<dyn Error>> {
        Ok(&self.entries.get(&label.to_uppercase()).ok_or_else(|| format!("unknown/ambiguous package label: {label}"))?.0)
    }

    /// The file's bytes after checking its decoded identity.
    pub fn read(&self, label: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let (path, sha) = self.entries.get(&label.to_uppercase()).ok_or_else(|| format!("unknown/ambiguous package label: {label}"))?;
        let bytes = crate::bounded_read(path)?;
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if &got != sha {
            return Err(format!("decoded source identity differs: {label}").into());
        }
        Ok(bytes)
    }
}
