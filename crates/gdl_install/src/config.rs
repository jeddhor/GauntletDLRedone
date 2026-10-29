//! The game's configuration for an installed data directory: the port of
//! tools/python/run_play.py's config generation (the same rules and keys;
//! paths relative to the data directory, "base": "config"). The Python
//! script stays the developer route over private/; `gdl_import config`
//! regenerates this one, and the two are compared in docs/research/release.md.
use gdl_formats::elf::Elf;
use gdl_formats::wdata::WorldData;
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::path::Path;

type R<T> = Result<T, Box<dyn Error>>;

/// The shipped name catalog and the extraction manifest's file names.
pub struct Names {
    /// Upper-case label → entry indices carrying it.
    by_label: HashMap<String, Vec<usize>>,
    /// Manifest output file by entry index.
    files: Vec<String>,
}

impl Names {
    pub fn new(catalog: &Value, files: Vec<String>) -> R<Self> {
        let mut by_label: HashMap<String, Vec<usize>> = HashMap::new();
        for e in catalog["entries"].as_array().ok_or("catalog entries")? {
            let index = e["index"].as_u64().ok_or("catalog index")? as usize;
            for n in e["proposed_names"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                by_label.entry(n.to_uppercase()).or_default().push(index);
            }
        }
        Ok(Self { by_label, files })
    }

    fn known(&self, label: &str) -> bool {
        self.by_label.contains_key(label)
    }

    /// run_play.py's resolve: the one entry carrying `label`, as
    /// "wad/<file>".
    fn resolve(&self, label: &str) -> R<String> {
        match self.by_label.get(label).map(Vec::as_slice) {
            Some([i]) => Ok(format!("wad/{}", self.files.get(*i).ok_or("manifest index")?)),
            _ => Err(format!("unknown/ambiguous package label: {label}").into()),
        }
    }

    fn labels(&self) -> BTreeSet<&str> {
        self.by_label.keys().map(String::as_str).collect()
    }
}

/// ELF worlddatas 0x23deb0: realm letter → (CurWorld, WDATA name).
const REALMS: [(char, i32, &str); 13] = [
    ('A', 1, "CASTLE"),
    ('B', 2, "MOUNT"),
    ('C', 3, "DESERT"),
    ('D', 4, "FOREST"),
    ('E', 5, "TEMPLE"),
    ('F', 6, "HELL"),
    ('G', 7, "TOWN"),
    ('H', 8, "BATTLE"),
    ('I', 9, "ICE"),
    ('J', 10, "DREAM"),
    ('K', 11, "SKY"),
    ('S', 12, "SECRET"),
    ('L', 0xd, "TOWER"),
];

/// LoadGauntWorldStart 0x163ec0: critter enemy types → CRITTER/*.WAD.
fn critter_file(kind: i32) -> Option<&'static str> {
    Some(match kind {
        0x21 => "GENERAL",
        0x22 => "DRAGON",
        0x23 => "CHIMERA",
        0x24 => "DJINN",
        0x25 => "DRIDER",
        0x26 => "PBOSS",
        0x27 => "YETI",
        0x28 => "WRAITH",
        0x29 => "LICH",
        0x2a => "SKORNE1",
        0x2b => "SKORNE2",
        0x2c => "GARM",
        _ => return None,
    })
}

fn cstr(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned()
}

/// A tagged chunk directory (WDATA, CRITTER): (tag, offset, count).
fn chunks(b: &[u8]) -> R<Vec<([u8; 4], usize, u32)>> {
    let word = |o: usize| -> R<u32> { Ok(u32::from_le_bytes(b.get(o..o + 4).ok_or("chunk directory")?.try_into()?)) };
    let (dir, count) = (word(0)? as usize, word(4)?);
    if count > 64 {
        return Err("chunk count".into());
    }
    (0..count as usize).map(|i| Ok((word(dir + 16 * i)?.to_be_bytes(), word(dir + 16 * i + 4)? as usize, word(dir + 16 * i + 8)?))).collect()
}

struct Builder<'a> {
    names: &'a Names,
    data: &'a Path,
    elf: &'a Elf<'a>,
    packages: Map<String, Value>,
    monsters: BTreeSet<(i32, String, String)>,
}

impl Builder<'_> {
    fn package(&self, prefix: &str, anim: bool) -> R<Value> {
        let mut out = Map::new();
        for (k, f) in [("objects", "OBJECTS"), ("texdef", "TEXDEF"), ("textures", "TEXTURES")] {
            out.insert(k.into(), Value::String(self.names.resolve(&format!("{prefix}/{f}.PS2"))?));
        }
        if anim {
            out.insert("anim".into(), Value::String(self.names.resolve(&format!("{prefix}/ANIM.PS2"))?));
        }
        Ok(Value::Object(out))
    }

    fn add_package(&mut self, key: &str, prefix: &str) -> R<()> {
        if !self.packages.contains_key(key) {
            let p = self.package(prefix, true)?;
            self.packages.insert(key.to_string(), p);
        }
        Ok(())
    }

    fn read(&self, rel: &str) -> R<Vec<u8>> {
        Ok(std::fs::read(self.data.join(rel))?)
    }

    /// level_monsters: GauntWorldInit's LoadEnemy types (enemy_table
    /// 0x22c7a0, GetEnemyDir 0x138118) present on the disc.
    fn level_monsters(&self, types: &gdl_core::level::EnemyTypes) -> R<Vec<(i32, String, String)>> {
        let mut table = HashMap::new();
        for k in 0..44u32 {
            let rec = self.elf.read(0x22c7a0 + 0x24 * k, 0x24)?;
            let t = i32::from_le_bytes(rec[..4].try_into()?);
            table.insert(t, (cstr(&rec[4..0x14]), cstr(&rec[0x14..0x24])));
        }
        let mut out: Vec<(i32, String, String)> = Vec::new();
        for (&t, &subtype) in types.kind.iter().zip(types.subtype.iter()) {
            if t < 0 || t == 0x1d || (0x20..=0x2c).contains(&t) || out.iter().any(|o| o.0 == t) {
                continue;
            }
            let Some((name, prefix)) = table.get(&t) else { continue };
            let name = name.to_uppercase();
            let dir = if subtype == 4 {
                format!("MONSTERS/{name}AUX")
            } else if subtype >= 11 {
                format!("MONSTERS/{name}{}", subtype - 10)
            } else {
                format!("MONSTERS/{name}")
            };
            if self.names.known(&format!("{dir}/ANIM.PS2")) {
                out.push((t, prefix.clone(), dir));
            }
        }
        Ok(out)
    }

    /// critter_files: [{wad, anim, lib}] for a level's critter types.
    fn critter_files(&self, types: &gdl_core::level::EnemyTypes, wdata: &WorldData, world: i32) -> R<Vec<Value>> {
        let mut gar = None;
        for (&k, d) in types.kind.iter().zip(types.data.iter()) {
            if let (0x20, Some(d)) = (k, d) {
                let raw = &wdata.enemies.get(*d).ok_or("ENMY index")?.raw;
                gar = Some(cstr(&raw[0x10..0x18]));
            }
        }
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for &k in &types.kind {
            if seen.contains(&k) {
                continue;
            }
            seen.push(k);
            let name = match k {
                0x1d => match world {
                    9 => "GOLEMI".to_string(),
                    6 => "GOLEMF".to_string(),
                    _ => "GOLEM".to_string(),
                },
                0x20 => match &gar {
                    Some(g) => format!("GAR_{}", g.to_uppercase()),
                    None => continue,
                },
                _ => match critter_file(k) {
                    Some(n) => n.to_string(),
                    None => continue,
                },
            };
            let wad = self.names.resolve(&format!("CRITTER/{name}.WAD"))?;
            let b = self.read(&wad)?;
            let mut desc = None;
            let mut kind = 0i16;
            for (tag, off, n) in chunks(&b)? {
                if &tag == b"DESC" && n != 0 {
                    desc = Some(cstr(b.get(off..off + 16).ok_or("DESC")?).to_uppercase());
                    kind = i16::from_le_bytes(b.get(off + 0x20..off + 0x22).ok_or("DESC kind")?.try_into()?);
                }
            }
            let mut entry = Map::new();
            entry.insert("wad".into(), Value::String(wad));
            if let Some(desc) = desc {
                let model = match kind {
                    7 => format!("MONSTERS/{desc}_{}/ANIM.PS2", gar.as_deref().ok_or("kind 7 without a gargoyle name")?.to_uppercase()),
                    3 | 8 => format!("MONSTERS/{desc}/{}/ANIM.PS2", wdata.world_dir.as_deref().ok_or("WRLD directory")?.to_uppercase()),
                    _ => format!("MONSTERS/{desc}/ANIM.PS2"),
                };
                if !desc.is_empty() && self.names.known(&model) {
                    entry.insert("anim".into(), Value::String(self.names.resolve(&model)?));
                    let lib = model.rsplit_once('/').map_or("", |(l, _)| l).to_string();
                    if self.names.known(&format!("{lib}/OBJECTS.PS2")) {
                        entry.insert("lib".into(), Value::String(lib));
                    }
                }
            }
            out.push(Value::Object(entry));
        }
        Ok(out)
    }

    fn realm_levels(&mut self, letter: char) -> R<Vec<Value>> {
        let &(_, world, wad) = REALMS.iter().find(|r| r.0 == letter).ok_or("realm")?;
        let wdata_path = self.names.resolve(&format!("WDATA/{wad}.WAD"))?;
        let wdata = WorldData::parse(&self.read(&wdata_path)?)?;
        let mut records: Vec<(usize, String, i32)> = wdata.levels.iter().map(|l| (l.index as usize, l.code.clone(), l.boss_type)).collect();
        records.sort_by_key(|r| (0 <= r.2, r.1[1..].parse::<i32>().unwrap_or(0)));
        // Records without level files (TOWER.WAD's L2) are skipped.
        records.retain(|r| self.names.known(&format!("LEVELS/LEVEL{}/WORLDS.PS2", r.1)));
        let title = match letter {
            'G' => "The Poison Fields".to_string(),
            'L' => "The Tower".to_string(),
            'S' => "Treasure room".to_string(),
            _ => {
                let mut t = wad.to_lowercase();
                t[..1].make_ascii_uppercase();
                format!("{t} realm ({letter})")
            }
        };
        let mut out = Vec::new();
        for (index, code, boss) in records {
            // LoadItems 0x14f7d0: items/<WorldItemDesc> while BossType < 0,
            // then items/level<code>.
            let mut items = Vec::new();
            for (x, used) in [(letter.to_string(), boss < 0), (code.clone(), true)] {
                if used && self.names.known(&format!("ITEMS/LEVEL{x}/ANIM.PS2")) {
                    self.add_package(&format!("ITEMS_{x}"), &format!("ITEMS/LEVEL{x}"))?;
                    items.push(Value::String(format!("ITEMS_{x}")));
                }
            }
            let level = &wdata.levels[index];
            let types = gdl_core::level::enemy_types(&wdata, level, 0, 3)?;
            let level_m: Vec<(i32, String, String)> =
                self.level_monsters(&types)?.into_iter().map(|(t, prefix, d)| (t, prefix, d.split_once('/').map_or(d.clone(), |(_, n)| n.to_string()))).collect();
            for (t, prefix, name) in &level_m {
                self.monsters.insert((*t, name.clone(), prefix.clone()));
                self.add_package(name, &format!("MONSTERS/{name}"))?;
            }
            let critters = self.critter_files(&types, &wdata, world)?;
            for c in &critters {
                if let Some(lib) = c["lib"].as_str() {
                    self.add_package(lib, lib)?;
                }
            }
            out.push(json!({
                "name": code,
                "index": index,
                "world": world,
                "wdata": wdata_path,
                "title": title,
                "worlds": self.names.resolve(&format!("LEVELS/LEVEL{code}/WORLDS.PS2"))?,
                "level": self.package(&format!("LEVELS/LEVEL{code}"), self.names.known(&format!("LEVELS/LEVEL{code}/ANIM.PS2")))?,
                "items": items,
                "monsters": level_m.iter().map(|m| m.2.clone()).collect::<Vec<_>>(),
                "monster_types": level_m.iter().map(|(t, p, n)| json!({"type": t, "package": n, "prefix": p})).collect::<Vec<_>>(),
                "critters": critters,
            }));
        }
        Ok(out)
    }
}

/// The configuration of the data directory `data` (wad/, disc/ and
/// catalog.json written; `elf` its disc/SLUS_200.47). `movies`: the
/// disc's MPEG and MPEG2 file names copied to disc/.
pub fn generate(data: &Path, names: &Names, elf: &Elf<'_>, movies: &[String]) -> R<Value> {
    let mut b = Builder { names, data, elf, packages: Map::new(), monsters: BTreeSet::new() };
    for p in ["POWERUPS", "WEAPONS", "STATIC"] {
        let v = b.package(p, true)?;
        b.packages.insert(p.to_string(), v);
    }
    // The Tower (hub) first, then every realm and world 12's treasure rooms.
    let mut levels = b.realm_levels('L')?;
    for letter in "ABCDEFGHIJKS".chars() {
        levels.extend(b.realm_levels(letter)?);
    }
    // audio_config: AUDATPS2.ROM, AUDIO/*.VBK banks, STREAMS/*.ADS, DCS.IRX.
    let labels = names.labels();
    let mut banks = Map::new();
    let mut streams = Map::new();
    for l in &labels {
        if let Some(n) = l.strip_prefix("AUDIO/").and_then(|n| n.strip_suffix(".VBK")) {
            banks.insert(n.to_string(), Value::String(names.resolve(l)?));
        }
        if let Some(n) = l.strip_prefix("STREAMS/").filter(|n| n.ends_with(".ADS")) {
            streams.insert(n.to_string(), Value::String(names.resolve(l)?));
        }
    }
    let mut audio = json!({"rom": names.resolve("AUDIO/AUDATPS2.ROM")?, "banks": banks, "streams": streams});
    if data.join("disc/IRX/DCS.IRX").exists() {
        audio["dcs"] = Value::String("disc/IRX/DCS.IRX".into());
    }
    let mut movie_map = Map::new();
    for m in movies {
        if let Some((dir, file)) = m.split_once('/')
            && let Some(stem) = file.strip_suffix(".PSS")
        {
            movie_map.insert(format!("{dir}/{stem}"), Value::String(format!("disc/{m}")));
        }
    }
    let monsters: Vec<Value> = b.monsters.iter().map(|(t, n, p)| json!({"type": t, "package": n, "prefix": p})).collect();
    let packages = std::mem::take(&mut b.packages);
    Ok(json!({
        "base": "config",
        "elf": "disc/SLUS_200.47",
        "levels": levels,
        "catalog": {"names": "catalog.json", "archive": "wad/manifest.json", "root": "wad"},
        "packages": packages,
        "monsters": monsters,
        "audio": audio,
        "movies": movie_map,
    }))
}

/// Unused-label check for tests: every label the config names exists.
#[allow(dead_code)]
pub fn referenced(v: &Value, out: &mut HashSet<String>) {
    match v {
        Value::String(s) if s.starts_with("wad/") || s.starts_with("disc/") => {
            out.insert(s.clone());
        }
        Value::Array(a) => a.iter().for_each(|x| referenced(x, out)),
        Value::Object(o) => o.values().for_each(|x| referenced(x, out)),
        _ => {}
    }
}
