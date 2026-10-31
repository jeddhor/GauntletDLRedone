//! NATIVE ENHANCEMENT (HD textures, graphics plan steps 8–10): the census
//! of every texture the client can draw from the disc's packages, named as
//! texture_pack names them (the decoded level 0 that materials_for and
//! hud_image hash), with the draws that use it — the generator's input and
//! what decides which textures keep their originals. Bevy-free: it reads
//! the catalog's OBJECTS packages (with TEXDEF, TEXTURES, ANIM and a
//! level's WORLDS) and the ELF's particle presets.
use crate::catalog::Catalog;
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable, world::World};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;

/// The front-end and HUD packages hud_image draws from (STATIC, TITLE and
/// SELECT, the attract screens): every record also by hud_image's decoding.
pub const FRONT_PACKAGES: [&str; 6] = ["STATIC", "TITLE", "SELECT", "SONY", "GROUP1", "CREDITS"];

/// What draws a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum UseKind {
    /// A level's world node (WORLDS placements): `flags` its MB flags
    /// (CreateWorldNode).
    World,
    /// A package tree's node object or OANIM frame object: `flags` its MB
    /// flags (AtreeNodeInit).
    Tree,
    /// A particle system's sprite texture (its record's name, or
    /// PARTICLE2_A / PARTICLE2_XP), matched by TEXDEF label: `flags` the
    /// system's node flags.
    Particle,
    /// A texture modifier's frame record: `who` its target's name.
    Frame,
    /// A front-end or HUD package's record (hud_image).
    Hud,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Use {
    pub kind: UseKind,
    pub flags: u32,
    pub package: String,
    /// The node, tree or system label; a frame's target texture name.
    pub who: String,
    /// A world or tree part with a lightmap record (format 146).
    pub lightmapped: bool,
    /// A world node's radius (WorldNode::radius), rounded.
    pub radius: i32,
}

#[derive(Debug, Default, Serialize)]
pub struct Entry {
    pub name: String,
    pub label: String,
    pub width: u16,
    pub height: u16,
    /// The bitmap record's flags (0x4 / 0x8: clamp U / V).
    pub flags: u16,
    /// Level 0's alpha class as materials_for reads it: 0 opaque, 1 cut-out
    /// (texels rejected or full), 2 blended.
    pub class: u8,
    /// Only hud_image decodes it (a front package record materials_for
    /// leaves as a placeholder, or a format 146/130 glyph page).
    pub hud_only: bool,
    /// (package, bitmap record) pairs holding these texels.
    pub sources: BTreeSet<(String, u16)>,
    pub uses: BTreeSet<Use>,
}

/// A package's files by kind (OBJECTS, TEXDEF, TEXTURES, ANIM, WORLDS).
struct Files {
    objects: Objects,
    table: TextureTable,
    textures: Vec<u8>,
    anim: Option<(Animations, Vec<u8>)>,
    world: Option<(World, Vec<u8>)>,
}

fn load(catalog: &Catalog, prefix: &str) -> Result<Files, Box<dyn Error>> {
    let read = |f: &str| catalog.read(&format!("{prefix}/{f}"));
    let objects = Objects::parse(&read("OBJECTS.PS2")?)?;
    let table = TextureTable::parse(&read("TEXDEF.PS2")?)?;
    let textures = read("TEXTURES.PS2")?;
    let anim = match read("ANIM.PS2") {
        Ok(b) => Animations::parse(&b).ok().map(|a| (a, b)),
        Err(_) => None,
    };
    let world = match read("WORLDS.PS2") {
        Ok(b) => Some((World::parse(&b)?, b)),
        Err(_) => None,
    };
    Ok(Files { objects, table, textures, anim, world })
}

/// Level 0's alpha class (materials_for's rule).
pub fn alpha_class(rgba: &[u8]) -> u8 {
    if rgba.chunks_exact(4).all(|p| p[3] >= 0x80) {
        0
    } else if rgba.chunks_exact(4).all(|p| p[3] <= 1 || p[3] >= 0x80) {
        1
    } else {
        2
    }
}

/// One record's census name and texels by materials_for's decoding.
pub fn material_texels(objects: &Objects, table: &TextureTable, textures: &[u8], record: usize) -> Option<(String, Vec<u8>)> {
    let b = objects.bitmap_records.get(record)?;
    if b.format == 146 {
        return None;
    }
    let def = crate::record_texdef(table, b)?;
    let mut rgba = def.decode_level_rgba(textures, 0).ok()?;
    crate::texa_alpha(b.format, &mut rgba);
    Some((def.label.clone(), rgba))
}

/// A particle system's texture label (setupParms: the record's name, or the
/// default particle for its blend).
fn psys_label(record: &[u8], presets: &[u8], dir: Option<[f32; 3]>, world: bool) -> Option<String> {
    let p = gdl_core::psys::Psys::new_world(0, record, presets, dir, world).ok()?;
    Some(p.texture.clone().map(|t| t.to_uppercase()).unwrap_or_else(|| {
        if p.node_flags & 0x800000 != 0 { "PARTICLE2_XP".into() } else { "PARTICLE2_A".into() }
    }))
}

/// Every texture of the catalog's OBJECTS packages with its uses. `presets`
/// is the ELF's preset_list (world particle systems).
pub fn census(catalog: &Catalog, presets: &[u8]) -> Result<Vec<Entry>, Box<dyn Error>> {
    let prefixes: Vec<String> = catalog.labels().iter().filter_map(|l| l.strip_suffix("/OBJECTS.PS2")).map(str::to_string).collect();
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    // Particle texture labels with their uses, matched once every package's
    // labels are known.
    let mut particles: Vec<(String, Use)> = Vec::new();
    for prefix in &prefixes {
        let Ok(f) = load(catalog, prefix) else { continue };
        // MAPS/level<code>: the realm map screen's texture set (start_map).
        let front = FRONT_PACKAGES.contains(&prefix.as_str()) || prefix.starts_with("MAPS/");
        // Record → census name.
        let mut names: HashMap<usize, String> = HashMap::new();
        for (i, b) in f.objects.bitmap_records.iter().enumerate() {
            let material = material_texels(&f.objects, &f.table, &f.textures, i);
            let hud = if front && material.is_none() { crate::hud_rgba(&f.objects, &f.table, &f.textures, i) } else { None };
            let (label, rgba, hud_only) = match (material, hud) {
                (Some((label, rgba)), _) => (label, rgba, false),
                (None, Some(rgba)) => {
                    let label = f.objects.bitmap_definitions.iter().find(|d| d.bitmap_index as usize == i).map_or(String::new(), |d| d.label.clone());
                    (label, rgba, true)
                }
                (None, None) => continue,
            };
            let name = crate::texture_pack::name(b.width, b.height, &rgba);
            let e = entries.entry(name.clone()).or_insert_with(|| Entry {
                name: name.clone(),
                label: label.clone(),
                width: b.width,
                height: b.height,
                flags: b.flags,
                class: alpha_class(&rgba),
                hud_only,
                ..Default::default()
            });
            e.hud_only &= hud_only;
            e.sources.insert((prefix.clone(), i as u16));
            if front {
                e.uses.insert(Use { kind: UseKind::Hud, flags: 0, package: prefix.clone(), who: label, lightmapped: false, radius: 0 });
            }
            names.insert(i, name);
        }
        // Object index → its parts' texture names, each with whether the
        // part has a lightmap record.
        let lightmap = |i: u16| f.objects.bitmap_records.get(i as usize).is_some_and(|b| b.format == 146);
        let object_names = |o: usize| -> Vec<(String, bool)> {
            f.objects.objects.get(o).map_or(Vec::new(), |o| {
                o.parts.iter().filter_map(|p| names.get(&(p.texture_index as usize)).map(|n| (n.clone(), lightmap(p.lightmap_index)))).collect()
            })
        };
        let mut by_label: HashMap<String, usize> = HashMap::new();
        let mut index_of: HashMap<String, usize> = HashMap::new();
        for d in &f.objects.definitions {
            if let Ok(o) = usize::try_from(d.object_index) {
                by_label.entry(d.label.clone()).or_insert(o);
                index_of.entry(d.label.to_uppercase()).or_insert(o);
            }
        }
        let mut add = |name: &str, u: Use| {
            if let Some(e) = entries.get_mut(name) {
                e.uses.insert(u);
            }
        };
        // World nodes (spawn_level): an object on non-particle nodes with
        // +0x28 == 1 or file flag 0x80000000; MB flags = record +0x18, | 4
        // for file flags & 0x1001 == 1, | 1 for file flag 0x8000.
        if let Some((world, bytes)) = &f.world
            && let Ok(placements) = world.placements()
        {
            let header = |i: usize| bytes.get(i * 4..i * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
            let version = header(0x18);
            let table: &[u8] = if version & 0xf00bab00 == 0xf00bab00 && 2 <= version & 0xff {
                let (count, offset) = (header(0x1c) as usize, header(0x1d) as usize);
                bytes.get(offset..offset + count * gdl_core::psys::RECORD).unwrap_or_default()
            } else {
                &[]
            };
            for p in &placements {
                let node = &world.nodes[p.node_index as usize];
                if node.flags & 0x800 != 0 {
                    // World particle systems (spawn_world_psys).
                    if node.flags & 0x1000_0000 != 0 {
                        continue;
                    }
                    let Some(at) = node.label.find("PSYS") else { continue };
                    let Some(letter) = node.label.as_bytes().get(at + 4).copied() else { continue };
                    let Some(record) = table.chunks_exact(gdl_core::psys::RECORD).find(|r| r[6] == letter) else { continue };
                    let dir = (0 < node.collision_count).then(|| world.collisions.get(node.collision_index as usize)).flatten().map(|c| c.normal.map(|v| -v));
                    if let Some(label) = psys_label(record, presets, dir, node.flags & 0x1000 != 0) {
                        let flags = gdl_core::psys::Psys::new_world(0, record, presets, dir, node.flags & 0x1000 != 0).map_or(0, |p| p.node_flags);
                        particles.push((label, Use { kind: UseKind::Particle, flags, package: prefix.clone(), who: node.label.clone(), lightmapped: false, radius: 0 }));
                    }
                    continue;
                }
                let has_object = i32::from_le_bytes(node.raw[40..44].try_into().unwrap()) == 1 || node.flags & 0x80000000 != 0;
                if !has_object {
                    continue;
                }
                let mut flags = node.render_flags;
                if node.flags & 0x1001 == 1 {
                    flags |= 4;
                }
                if node.flags & 0x8000 != 0 {
                    flags |= 1;
                }
                let Some(&o) = by_label.get(&node.label) else { continue };
                for (name, lightmapped) in object_names(o) {
                    add(&name, Use { kind: UseKind::World, flags, package: prefix.clone(), who: node.label.clone(), lightmapped, radius: node.radius.round() as i32 });
                }
            }
        }
        // Package trees (tree_node_model): the node's named object, its
        // particle system (type 4) or its OANIM frame objects (type 2).
        if let Some((anim, bytes)) = &f.anim {
            for tree in &anim.trees {
                for node in &tree.nodes {
                    let mut flags = node.model_flags;
                    if node.node_type != 2 && (node.label == "DUMMY" || node.flags & 1 != 0) {
                        flags |= 1;
                    }
                    let who = format!("{}:{}", tree.label, node.label);
                    let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
                    if node.flags & 1 == 0 && node.node_type != 2 && let Some(&o) = by_label.get(&label) {
                        for (name, lightmapped) in object_names(o) {
                            add(&name, Use { kind: UseKind::Tree, flags, package: prefix.clone(), who: who.clone(), lightmapped, radius: 0 });
                        }
                    } else if node.node_type == 4 && 0x38 <= node.raw.len() {
                        let base = tree.file_offset as usize;
                        let Some(h0) = bytes.get(base..base + 4).map(|v| u32::from_le_bytes(v.try_into().unwrap()) as i64) else { continue };
                        let Ok(at) = usize::try_from(base as i64 + h0 + i64::from(node.sequence_info_offset)) else { continue };
                        let Some(record) = bytes.get(at..at + gdl_core::psys::RECORD) else { continue };
                        let fl = |o: usize| f32::from_le_bytes(node.raw[o..o + 4].try_into().unwrap());
                        let dir = Some([fl(0x14), fl(0x18), fl(0x1c)]);
                        if let Some(label) = psys_label(record, &[], dir, true) {
                            let nf = gdl_core::psys::Psys::new_world(0, record, &[], dir, true).map_or(0, |p| p.node_flags);
                            particles.push((label, Use { kind: UseKind::Particle, flags: nf, package: prefix.clone(), who, lightmapped: false, radius: 0 }));
                        }
                    } else if node.node_type == 2 {
                        for e in &node.oanim {
                            let Some(&base) = index_of.get(&e.name.to_uppercase()) else { continue };
                            for k in 0..e.count.max(1) as usize {
                                for (name, lightmapped) in object_names(base + k) {
                                    add(&name, Use { kind: UseKind::Tree, flags, package: prefix.clone(), who: who.clone(), lightmapped, radius: 0 });
                                }
                            }
                        }
                    }
                }
            }
            // Texture modifiers: frame records of this package.
            for tm in gdl_formats::animation::texmods(bytes).unwrap_or_default() {
                let target = usize::try_from(tm.target).ok().and_then(|t| names.get(&t)).cloned().unwrap_or_else(|| tm.target_name.to_uppercase());
                let Ok(first) = usize::try_from(tm.frame_base) else { continue };
                for k in 0..tm.count.unsigned_abs() as usize {
                    if let Some(name) = names.get(&(first + k)) {
                        add(name, Use { kind: UseKind::Frame, flags: 0, package: prefix.clone(), who: target.clone(), lightmapped: false, radius: 0 });
                    }
                }
            }
        }
    }
    // Particle textures by TEXDEF label (MBOX_FindTexture searches by name).
    let mut by_label: HashMap<String, Vec<String>> = HashMap::new();
    for e in entries.values() {
        by_label.entry(e.label.to_uppercase()).or_default().push(e.name.clone());
    }
    for (label, u) in particles {
        for name in by_label.get(&label).into_iter().flatten() {
            if let Some(e) = entries.get_mut(name) {
                e.uses.insert(u.clone());
            }
        }
    }
    Ok(entries.into_values().collect())
}
