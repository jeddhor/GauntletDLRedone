//! Sound cues: the EE side of the DCS audio system. Each Audio* function is
//! transcribed to the sound ID it requests (AudioSFX 0x1227f0, AudioSFX2
//! 0x122880, AudioSFX3 0x1228e0 with AudioVolMul 0x121988, AudioFX 0x1229a8,
//! AudioQueAdd4 0x123170, AudioWithName 0x15d9f8) and emitted as
//! `Event::Sound`; playback (DCS.IRX BankPlay) is the client's job.
//!
//! IDs are `bank << 16 | call` (AUDATPS2.ROM). Runtime tables are rebuilt
//! natively: InitNameAudio 0x15d7b8 (announcer names), SetEnemyAudio
//! 0x15f1c8 per GetEnemyTypes slot (monster hurt/kill/bite), the level's
//! WDATA SNDS world sounds (PlayWorldSound 0x15c4f0) and AUDS stage record.
use super::enemies::e;
use super::sound_cues::CueTables;
use super::{Event, Game, R};
use crate::numeric::{div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
use gdl_formats::audio::AudioRom;
use gdl_formats::elf::Elf;

type V3 = [f32; 3];

/// One AUDS stage record (0x3c bytes): bank name, enter sound, stream.
#[derive(Debug, Clone, Default)]
pub struct Stage {
    pub bank: String,
    pub word_10: i16,
    /// Stage +0x12: world sound for AudioEffect 5 and AudioExplodeWall.
    pub enter_sound: i16,
    /// Stage +0x14: AudioEnterNextStage's second line (−1: none).
    pub word_14: i32,
    pub stream: String,
    pub areas: i16,
    pub word_2a: i16,
    pub parts: Vec<i16>,
}

impl Stage {
    /// BGPlayStream's stream name for an attention area and part (".ads").
    pub fn stream_name(&self, area: usize, part: usize) -> String {
        let mut s = self.stream.to_uppercase();
        if self.areas != 1 {
            s.push((b'A' + area as u8) as char);
        }
        if 2 <= self.parts.get(area).copied().unwrap_or(1) {
            s.push_str(&format!("_{}", part + 1));
        }
        s + ".ADS"
    }
}

#[derive(Debug, Clone, Default)]
pub struct SoundTables {
    pub rom: Option<AudioRom>,
    throw: Vec<i32>,
    pain: Vec<i32>,
    severe: Vec<i32>,
    eat: Vec<i32>,
    eatsfx: Vec<i32>,
    poison: Vec<i32>,
    stolen: Vec<i32>,
    dievox: Vec<i32>,
    dmg: Vec<i32>,
    potion: Vec<i32>,
    tile: Vec<i32>,
    barrel: Vec<i32>,
    /// snd_351 0x23d488 / snd_355 0x23d4c0: poison / explosive barrel per world.
    poison_barrel: Vec<i32>,
    /// snd1_370 0x23d568 / snd2_371 0x23d5a0: rotator loop / stop per world.
    rotator_loop: Vec<i32>,
    rotator_stop: Vec<i32>,
    /// Loop IDs started and not yet killed (AudioSoundExists).
    pub loops: std::collections::BTreeSet<i32>,
    barrel_explode: Vec<i32>,
    rock: Vec<i32>,
    leaf: Vec<i32>,
    door: Vec<i32>,
    gen_die: Vec<i32>,
    gen_hit: Vec<i32>,
    pantab: Vec<i32>,
    /// enemy_idx by enemy type → GetEnemyTypes slot.
    enemy_idx: Vec<i32>,
    /// monkill1, monkill1L3, monkill2, monkill2L3, monhurt1, monhurt2,
    /// monhurt1L3D0, monhurt1L3D1, monhurt2L3D0, monhurt2L3D1, monbite, monbiteL3.
    mon: [[i32; 8]; 12],
    /// snd_pname1/2 by colour × character (InitNameAudio; colour sounds are −1).
    pname: [[[i32; 17]; 4]; 2],
    /// WDATA SNDS: (id, volume, priority).
    pub world: Vec<(i32, i16, i16)>,
    /// The sound_cues.rs tables (EE addresses in its doc).
    pub cues: CueTables,
    pub stage: Option<Stage>,
}

fn words(b: &[u8]) -> Vec<i32> {
    b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
}

impl SoundTables {
    /// `enemies`: GetEnemyTypes (type, subtype, ENMY name) per slot.
    pub fn new(
        elf: &Elf,
        rom: Option<AudioRom>,
        enemies: &[(i32, i32, String)],
        boss_type: i32,
        snds: &[(String, i16, i16)],
        stage: Option<Stage>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let t = |a: u32, n: u32| -> Result<Vec<i32>, Box<dyn std::error::Error>> { Ok(words(elf.read(a, n)?)) };
        let mut s = Self {
            throw: t(0x23_d0c0, 32)?,
            pain: t(0x23_cf00, 128)?,
            severe: t(0x23_cf80, 32)?,
            eat: t(0x23_cfa0, 128)?,
            eatsfx: t(0x23_d020, 32)?,
            poison: t(0x23_d040, 32)?,
            stolen: t(0x23_d060, 32)?,
            dievox: t(0x23_d228, 32)?,
            dmg: t(0x23_d1d8, 0x50)?,
            potion: t(0x23_d3d0, 40)?,
            tile: t(0x23_d248, 392)?,
            barrel: t(0x23_d450, 56)?,
            poison_barrel: t(0x23_d488, 56)?,
            rotator_loop: t(0x23_d568, 56)?,
            rotator_stop: t(0x23_d5a0, 56)?,
            loops: Default::default(),
            barrel_explode: t(0x23_d4c0, 56)?,
            rock: t(0x23_d4f8, 56)?,
            leaf: t(0x23_d530, 56)?,
            door: t(0x23_d6b8, 224)?,
            gen_die: t(0x23_db70, 56)?,
            gen_hit: t(0x23_dba8, 56)?,
            pantab: t(0x23_ccc0, 20)?,
            cues: CueTables {
                turbo_a: t(0x23_d0e0, 128)?,
                turbo_b: t(0x23_d160, 32)?,
                turbo_c: t(0x23_d180, 32)?,
                walk: t(0x23_d3f8, 40)?,
                click2: t(0x23_d428, 8)?,
                map_dot: t(0x23_ce30, 56)?,
                tally: t(0x23_ce68, 56)?,
                runes: t(0x23_cea0, 44)?,
                transport: t(0x23_dac8, 56)?,
                world_explosion: t(0x23_db00, 56)?,
                world_hit: t(0x23_db38, 56)?,
                lvl_snd: t(0x23_d7c8, 576)?,
                gwiz: t(0x23_dbe0, 352)?,
                legend: [t(0x23_da08, 48)?, t(0x23_da38, 48)?, t(0x23_da68, 48)?, t(0x23_da98, 48)?],
                ricochet_time: 0.0,
            },
            enemy_idx: vec![-1; 0x2d],
            mon: [[-1; 8]; 12],
            pname: [[[-1; 17]; 4]; 2],
            world: Vec::new(),
            stage,
            rom: None,
        };
        let Some(rom) = rom else { return Ok(s) };
        let find = |n: &str| rom.find(n);
        // InitNameAudio: S_<COLOUR><CHAR>1 / 2 (colour words stay −1).
        let colours = ["YEL", "BLU", "RED", "GRE"];
        let chars = ["WAR", "VAL", "WIZ", "ARC", "DWF", "KNI", "SOR", "JES", "MIN", "FAL", "JAC", "TIG", "OGR", "UNI", "MED", "HYE", "SUM"];
        for (c, col) in colours.iter().enumerate() {
            for (k, ch) in chars.iter().enumerate().take(16) {
                s.pname[0][c][k] = find(&format!("S_{col}{ch}1")).unwrap_or(-1);
                s.pname[1][c][k] = find(&format!("S_{col}{ch}2")).unwrap_or(-1);
            }
        }
        // SetEnemyAudio for each GetEnemyTypes slot (subtypes 5 and 9 skipped).
        for (slot, (ty, sub_, name)) in enemies.iter().enumerate().take(8) {
            if *sub_ == 9 || *sub_ == 5 || *ty < 0 {
                continue;
            }
            s.enemy_idx[*ty as usize] = if name.is_empty() { -1 } else { slot as i32 };
            let leveled = matches!(ty, 1 | 2 | 4 | 5 | 7 | 8 | 10 | 0xb | 0xd | 0xe | 0x10 | 0x11 | 0x13 | 0x14 | 0x17 | 0x18 | 0x19 | 0x1a);
            let (b, n, mode, bite, kill) = if leveled {
                let (b, n, m) = if *sub_ < 10 && boss_type < 0 { (format!("{name}1"), format!("{name}2"), 1) } else { (format!("{name}2"), format!("{name}2"), 2) };
                let bite = if matches!(ty, 2 | 8 | 0x13 | 0x11 | 0x18 | 0x19) {
                    1
                } else if *ty == 0xb {
                    2
                } else {
                    0
                };
                (b, n, m, bite, "DIE")
            } else if *ty == 0x1b {
                (format!("{name}1"), format!("{name}1"), 0, 0, "DIE")
            } else if *ty == 0x1d {
                return Err("SetEnemyAudio golem (LevelLetter) not ported".into());
            } else {
                (name.clone(), name.clone(), 0, i32::from(*ty != 0x1d), "DIE")
            };
            // modify_boss_ene 0x15f168: on boss levels the name is cut at 14
            // characters and gets the boss letter (0x24 C, 0x25 D, 0x29 B).
            let modify = |n: String| -> String {
                let letter = match boss_type {
                    0x24 => "C",
                    0x25 => "D",
                    0x29 => "B",
                    _ => return n,
                };
                let mut t: String = n.chars().take(14).collect();
                t.push_str(letter);
                t
            };
            let f = |n: String| find(&if 0 < boss_type { modify(n) } else { n }).unwrap_or(-1);
            s.mon[0][slot] = f(format!("S_{b}{kill}CLOSE"));
            s.mon[1][slot] = f(format!("S_{n}{kill}CLOSE"));
            s.mon[2][slot] = f(format!("S_{b}{kill}FAR"));
            s.mon[3][slot] = f(format!("S_{n}{kill}FAR"));
            if mode < 2 {
                s.mon[4][slot] = f(format!("S_{b}HITCLOSE"));
                s.mon[5][slot] = f(format!("S_{b}HITFAR"));
            }
            if mode == 0 {
                let c = f(format!("S_{n}HITCLOSE"));
                let fa = f(format!("S_{n}HITFAR"));
                s.mon[6][slot] = c;
                s.mon[7][slot] = c;
                s.mon[8][slot] = fa;
                s.mon[9][slot] = fa;
            } else {
                s.mon[6][slot] = f(format!("S_{n}HIT1CLOSE"));
                s.mon[7][slot] = f(format!("S_{n}HIT2CLOSE"));
                s.mon[8][slot] = f(format!("S_{n}HIT1FAR"));
                s.mon[9][slot] = f(format!("S_{n}HIT2FAR"));
            }
            let pat = match bite {
                2 => Some("STRIKE"),
                1 => Some("BITE"),
                _ => None,
            };
            if let Some(p) = pat {
                s.mon[10][slot] = f(format!("S_{b}{p}"));
                s.mon[11][slot] = f(format!("S_{n}{p}"));
            }
        }
        // WDATA SNDS names resolved by AudioFindSound.
        s.world = snds.iter().map(|(n, v, p)| (find(n).unwrap_or(-1), *v, *p)).collect();
        s.rom = Some(rom);
        Ok(s)
    }
}

impl Game {
    fn sound_on(&self) -> bool {
        self.sounds.rom.is_some()
    }

    /// AudioSFX(id, position, volume, priority): pan from AudioAng.
    pub fn sfx(&mut self, id: i32, pos: Option<V3>, volume: i32, priority: i32) {
        if self.sound_on() && 0 <= id {
            self.events.push(Event::Sound { id, position: pos, volume, priority, queue: false });
        }
    }

    /// AudioSFX2(id, pan, volume, priority): fixed pan (pantab), no position.
    pub fn sfx2(&mut self, id: i32, _pan: i32, volume: i32, priority: i32) {
        if self.sound_on() && 0 <= id {
            self.events.push(Event::Sound { id, position: None, volume, priority, queue: false });
        }
    }

    /// AudioSFX3: volume × AudioVolMul (1.4 − distance/50, clamped 0..1).
    pub fn sfx3(&mut self, id: i32, pos: V3, volume: i32, priority: i32) -> R<()> {
        if !self.sound_on() || id < 0 {
            return Ok(());
        }
        let d = self.distance_to_closest_player(pos)?;
        let k = sub(1.4, div(d, 50.0)?)?;
        let k = k.clamp(0.0, 1.0);
        if 0.0 < k {
            let v = mul(volume as f32, k)? as i32;
            self.events.push(Event::Sound { id, position: Some(pos), volume: v, priority, queue: false });
        }
        Ok(())
    }

    /// AudioLOOP(id, position, volume, priority) once (AudioSoundExists).
    pub(crate) fn loop_sound(&mut self, id: i32, pos: Option<V3>, volume: i32, priority: i32) {
        if self.sound_on() && 0 <= id && self.sounds.loops.insert(id) {
            self.events.push(Event::LoopSound { id, position: pos, volume, priority });
        }
    }

    /// AudioSoundKill(id).
    pub(crate) fn kill_sound(&mut self, id: i32) {
        if 0 <= id {
            self.sounds.loops.remove(&id);
            self.events.push(Event::StopSound { id });
        }
    }

    /// AudioQueAdd4 / AudioVQueAdd4: queued (serialised) voice line.
    pub(crate) fn queue(&mut self, id: i32, volume: i32, priority: i32) {
        if self.sound_on() && 0 <= id {
            self.events.push(Event::Sound { id, position: None, volume, priority, queue: true });
        }
    }

    fn class_of(&self, p: usize) -> usize {
        self.players[p].u32_(8) as usize
    }
    fn chicken(&self, p: usize) -> bool {
        self.players[p].u32_(0x138) & 0x400 != 0
    }
    fn tab(v: &[i32], k: usize) -> i32 {
        v.get(k).copied().unwrap_or(-1)
    }

    /// AudioWithName(delay, first, player, message, extra): colour/name vox.
    pub fn audio_with_name(&mut self, first: i32, p: usize, message: i32, extra: i32) {
        let name = if self.chicken(p) {
            if first != -1 { 0x1002a } else { 0x1002b }
        } else {
            let colour = self.players[p].u32_(4) as usize;
            let ch = self.players[p].u32_(0xc) as usize;
            let set = usize::from(first == -1);
            self.sounds.pname[set].get(colour).and_then(|c| c.get(ch)).copied().unwrap_or(-1)
        };
        for id in [first, name, message, extra] {
            if 0 <= id {
                self.queue(id, 0xe0, 2);
            }
        }
    }

    pub fn audio_player_dies(&mut self, p: usize) {
        if self.players[p].i32_(0xfc) != 1 {
            return;
        }
        let pos = self.players[p].v3(0x50);
        self.sfx(1, Some(pos), 0x7f, 8);
        let v = Self::tab(&self.sounds.dievox, self.class_of(p));
        let id = if self.chicken(p) { 0x62 } else { v };
        self.sfx(id, Some(pos), 0xe0, 7);
    }

    /// AudioPlayerPain: RandInt(4) chooses the line (consumes the game RNG).
    pub fn audio_player_pain(&mut self, p: usize) -> R<()> {
        if self.players[p].i32_(0xfc) != 1 {
            return Ok(());
        }
        let r = self.rng.rand_int(4).ok_or_else(|| super::Unsupported("RandInt(4)".into()))? as usize;
        let v = Self::tab(&self.sounds.pain, r + 4 * self.class_of(p));
        let id = if self.chicken(p) { 0x60 } else { v };
        self.queue(id, 0xe0, 100);
        Ok(())
    }

    pub fn audio_player_severe_pain(&mut self, p: usize) {
        let v = Self::tab(&self.sounds.severe, self.class_of(p));
        if self.players[p].i32_(0xfc) == 1 && self.players[p].u32_(0x134) & 0x1_0000 == 0 && 0 <= v {
            let id = if self.chicken(p) { 0x60 } else { v };
            let pos = self.players[p].v3(0x50);
            self.sfx(id, Some(pos), 0xe0, 0x3e);
        }
    }

    pub fn audio_need_food(&mut self, kind: i32, p: usize) {
        let m = match kind {
            1 => 0x1_0005,
            2 => 0x1_0003,
            3 => 0x2_0000,
            _ => 0x1_0004,
        };
        self.audio_with_name(-1, p, m, -1);
    }

    pub fn audio_player_hit(&mut self, p: usize, kind: i32) {
        if self.players[p].i32_(0xfc) == 1 {
            let id = Self::tab(&self.sounds.dmg, p + 4 * kind.max(0) as usize);
            let pos = self.players[p].v3(0x50);
            self.sfx(id, Some(pos), 0x7f, 0x4a);
        }
    }

    pub fn audio_player_poison(&mut self, p: usize) {
        let v = Self::tab(&self.sounds.poison, self.class_of(p));
        if 0 <= v {
            let id = if self.chicken(p) { 0x62 } else { v };
            self.queue(id, 0xc0, 0x42);
        }
    }

    pub fn audio_player_stolen(&mut self, p: usize) {
        let v = Self::tab(&self.sounds.stolen, self.class_of(p));
        if !self.chicken(p) && 0 <= v {
            self.queue(v, 0xc0, 0x6e);
        }
    }

    /// AudioPlayerEatSFX: RandInt(4) == 0 speaks AudioPlayerEatFood.
    pub fn audio_player_eat_sfx(&mut self, p: usize, which: i32) -> R<()> {
        let r = self.rng.rand_int(4).ok_or_else(|| super::Unsupported("RandInt(4)".into()))?;
        if r == 0 {
            let v = Self::tab(&self.sounds.eat, which.max(0) as usize + 4 * self.class_of(p));
            if !self.chicken(p) && 0 <= v {
                self.queue(v, 0xc0, 0x42);
            }
            return Ok(());
        }
        let v = Self::tab(&self.sounds.eatsfx, self.class_of(p));
        if 0 <= v {
            let id = if self.chicken(p) { 0x61 } else { v };
            self.queue(id, 0xc0, 0x42);
        }
        Ok(())
    }

    pub fn audio_gained_level(&mut self, p: usize, dir: i32) {
        if 0 < dir {
            self.audio_with_name(-1, p, 0x2_0010, 0x1_003d);
        } else if dir < 0 {
            self.audio_with_name(-1, p, 0x1_003e, -1);
        }
    }

    pub fn audio_pickup(&mut self, p: usize) {
        let pan = Self::tab(&self.sounds.pantab, p);
        self.sfx2(0x26, pan, 0x7f, 0x42);
    }
    pub fn audio_key(&mut self, p: usize) {
        let pan = Self::tab(&self.sounds.pantab, p);
        self.sfx2(0x25, pan, 0x7f, 0x42);
    }
    /// AudioPickupGold 0x1609b8(player, amount): 0x26, in a treasure room
    /// the player's coin voice by value (bronze.493 for 50, silver.494 for
    /// 100, gold.495 otherwise: 0x3b0000 + 3 × player + 0/1/2).
    pub fn audio_pickup_gold(&mut self, p: usize, amount: i32) -> R<()> {
        let id = if self.globals.cur_world == 0xc {
            let k = match amount {
                0x32 => 0,
                100 => 1,
                _ => 2,
            };
            0x3b_0000 + 3 * (p as i32 & 3) + k
        } else {
            0x26
        };
        let pan = Self::tab(&self.sounds.pantab, p);
        self.sfx2(id, pan, 0x7f, 0x42);
        Ok(())
    }

    /// AudioPickupSpecial(player, subtype, flags).
    pub fn audio_pickup_special(&mut self, p: usize, subtype: i32, flags: u32) {
        let (mut id, mut vol) = (0x27, 0x7f);
        if subtype == 9 {
            if flags & 1 != 0 {
                id = 0x58;
            } else if flags & 0x100 != 0 {
                (id, vol) = (0x56, 0xb4);
            } else if flags & 0x200 != 0 {
                (id, vol) = (0x54, 0xb4);
            } else if flags & 0x400 != 0 {
                id = 0x5e;
            }
        } else if subtype == 6 && flags & 0x20_0000 != 0 {
            id = 0x29;
        } else if subtype == 0xf {
            id = 0x26;
        }
        let pan = Self::tab(&self.sounds.pantab, p);
        self.sfx2(id, pan, vol, 0x42);
    }

    /// AudioThrowWeapon(player): weapon powerup sounds, else the class throw.
    pub fn audio_throw_weapon(&mut self, p: usize) -> R<()> {
        let f = self.players[p].u32_(0x130);
        let pos = self.players[p].v3(0x50);
        let (id, prio) = if f & 0x58_0000 != 0 {
            (0x42, 0x28)
        } else {
            match f & 0xf {
                1 => (0x44, 0x28),
                2 => (0x46, 0x28),
                3 => (0x45, 0x28),
                4 => (0x43, 0x28),
                _ => (Self::tab(&self.sounds.throw, self.class_of(p)), 0x2a),
            }
        };
        self.sfx3(id, pos, 0x7f, prio)
    }

    pub fn audio_potion(&mut self, colour: usize, pos: V3, mode: usize) {
        let id = Self::tab(&self.sounds.potion, colour + 5 * mode);
        self.sfx(id, Some(pos), 0x7f, 0xf);
    }

    fn mon_slot(&self, i: usize) -> Option<usize> {
        let t = self.enemies[i].i32_(e::TYPE);
        usize::try_from(*self.sounds.enemy_idx.get(t as usize)?).ok()
    }

    pub fn audio_enemy_hurt(&mut self, i: usize, which: i32) -> R<()> {
        let Some(k) = self.mon_slot(i) else { return Ok(()) };
        let lvl = self.enemies[i].i16_(0x21a);
        let d = self.enemies[i].i16_(0x218);
        let table = match (which == 1, lvl < 2, d < 2) {
            (true, true, _) => 4,
            (true, false, true) => 6,
            (true, false, false) => 7,
            (false, true, _) => 5,
            (false, false, true) => 8,
            (false, false, false) => 9,
        };
        let id = self.sounds.mon[table][k];
        let pos = self.enemies[i].v3(e::POSITION);
        self.sfx3(id, pos, 0xe0, if which == 1 { 0x23 } else { 0x5f })
    }

    pub fn audio_enemy_dies(&mut self, i: usize, which: i32) -> R<()> {
        let Some(k) = self.mon_slot(i) else { return Ok(()) };
        let l3 = 2 <= self.enemies[i].i16_(0x21a);
        let table = match (which == 1, l3) {
            (true, false) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (false, true) => 3,
        };
        let id = self.sounds.mon[table][k];
        let pos = self.enemies[i].v3(e::POSITION);
        self.sfx3(id, pos, 0xe0, if which == 1 { 0x19 } else { 0x55 })
    }

    pub fn audio_enemy_bite(&mut self, i: usize) -> R<()> {
        let Some(k) = self.mon_slot(i) else { return Ok(()) };
        let table = if self.enemies[i].i16_(0x21a) < 2 { 10 } else { 11 };
        let id = self.sounds.mon[table][k];
        let pos = self.enemies[i].v3(e::POSITION);
        self.sfx3(id, pos, 0xb4, 0x4b)
    }

    fn world_tab(&self, v: &[i32], stride: usize, k: usize) -> i32 {
        Self::tab(v, self.globals.cur_world.max(0) as usize * stride + k)
    }

    pub fn audio_wood_barrel(&mut self, pos: V3) -> R<()> {
        let id = self.world_tab(&self.sounds.barrel, 1, 0);
        self.sfx3(id, pos, 0xe0, 0x55)
    }
    /// AudioRotator 0x160020(mode, position): mode 0 keeps the world's loop
    /// running (AudioLOOP 0xe0/0x72; the AudioTrackPan update is
    /// presentation); otherwise kill it, and mode 2+ plays the stop sound.
    pub fn audio_rotator(&mut self, mode: i32, pos: V3) -> R<()> {
        let lp = self.world_tab(&self.sounds.rotator_loop, 1, 0);
        let stop = self.world_tab(&self.sounds.rotator_stop, 1, 0);
        let mode = if self.globals.start_of_stage != 0 || self.globals.end_of_stage != 0 { 0 } else { mode };
        if mode == 0 {
            if self.sound_on() && 0 <= lp && self.sounds.loops.insert(lp) {
                self.events.push(Event::LoopSound { id: lp, position: Some(pos), volume: 0xe0, priority: 0x72 });
            }
            return Ok(());
        }
        if self.sounds.loops.remove(&lp) {
            self.events.push(Event::StopSound { id: lp });
        }
        if 1 < mode {
            self.sfx(stop, Some(pos), 0xe0, 0);
        }
        Ok(())
    }

    /// AudioPoisonBarrel 0x15fed8.
    pub fn audio_poison_barrel(&mut self, pos: V3) -> R<()> {
        let id = self.world_tab(&self.sounds.poison_barrel, 1, 0);
        if id < 0 {
            return Ok(());
        }
        self.sfx3(id, pos, 0xe0, 0x55)
    }
    /// AudioBarrelExplode 0x15ff10.
    pub fn audio_barrel_explode(&mut self, pos: V3) -> R<()> {
        let id = self.world_tab(&self.sounds.barrel_explode, 1, 0);
        if id < 0 {
            return Ok(());
        }
        self.sfx3(id, pos, 0xe0, 0x55)
    }
    /// AudioExplodeWall 0x161068(position, health): the stage record's
    /// +0x12 world sound while standing, else 0x2e.
    pub fn audio_explode_wall(&mut self, pos: V3, health: i32) -> R<()> {
        if 0 < health {
            let k = self.sounds.stage.as_ref().map_or(-1, |s| i32::from(s.enter_sound));
            return self.play_world_sound(k, pos);
        }
        self.sfx3(0x2e, pos, 0x7f, 0x18)
    }
    pub fn audio_rock_fall(&mut self, pos: V3) -> R<()> {
        let w = self.globals.cur_world;
        let id = if w == 6 && self.globals.cur_level == 1 {
            0x39_0000
        } else if w == 9 && self.globals.cur_level == 4 {
            0x32_000d
        } else {
            self.world_tab(&self.sounds.rock, 1, 0)
        };
        self.sfx3(id, pos, 0xe0, 0x12)
    }
    pub fn audio_leaf_fall(&mut self, pos: V3) -> R<()> {
        let id = self.world_tab(&self.sounds.leaf, 1, 0);
        self.sfx3(id, pos, 0xe0, 0x12)
    }
    pub fn audio_door(&mut self, pos: V3, kind: i32) -> R<()> {
        let id = self.world_tab(&self.sounds.door, 4, kind.max(0) as usize);
        self.sfx3(id, pos, 0x7f, 0x40)
    }
    pub fn audio_damage_tile(&mut self, pos: V3, kind: i32) {
        if !(0..7).contains(&kind) {
            return;
        }
        let mut id = self.world_tab(&self.sounds.tile, 7, kind as usize);
        if id == 0x26_0027 && self.globals.boss_type == 0x22 {
            id = 0x2e_0009;
        }
        let vol = if id == 0x25_0002 || id == 0x28_002e { 0xb4 } else { 0x7f };
        self.sfx(id, Some(pos), vol, 0x32);
    }
    pub fn audio_generator_dies(&mut self, pos: V3, ty: i32) -> R<()> {
        // CurWorld 10 type-0x18 generators use sound 0x2c0025 (lui/ori literal).
        let id = if self.globals.cur_world == 10 && ty == 0x18 { 0x2c_0025 } else { self.world_tab(&self.sounds.gen_die, 1, 0) };
        if self.globals.boss_type < 0 && 0 <= id {
            self.sfx3(id, pos, 0x7f, 0x51)?;
        }
        Ok(())
    }
    pub fn audio_generator_damaged(&mut self, pos: V3, ty: i32) -> R<()> {
        // CurWorld 10 type-0x18 generators use sound 0x2c0024 (lui/ori literal).
        let id = if self.globals.cur_world == 10 && ty == 0x18 { 0x2c_0024 } else { self.world_tab(&self.sounds.gen_hit, 1, 0) };
        self.sfx3(id, pos, 0xb4, 0x5b)
    }

    /// PlayWorldSound(index, position): WDATA SNDS entry.
    pub fn play_world_sound(&mut self, k: i32, pos: V3) -> R<()> {
        let Some(&(id, v, p)) = usize::try_from(k).ok().and_then(|k| self.sounds.world.get(k)) else { return Ok(()) };
        if id < 0 {
            return Ok(());
        }
        self.sfx3(id, pos, if v == 0 { 0xe0 } else { i32::from(v) }, if p == 0 { 0x7e } else { i32::from(p) })
    }

    /// AudioEffect(code, position).
    pub fn audio_effect(&mut self, code: i32, pos: V3) -> R<()> {
        let (id, vol, prio) = match code {
            1 => (0x30_000b, 0xe0, 0xe),
            2 => (0x39, 0xe0, 0x36),
            3 => (0x30_000d, 0xe0, 0xe),
            4 => (0x38, 0xe0, 0x32),
            5 => {
                let k = self.sounds.stage.as_ref().map_or(-1, |s| i32::from(s.enter_sound));
                return self.play_world_sound(k, pos);
            }
            6 => (0x3f, 0x7f, 0x28),
            7 => (0x40, 0x7f, 0x28),
            8..=0xb => (Self::tab(&self.sounds.potion, (code - 7) as usize), 0x7f, 0xf),
            0xc => (0x31_0011, 0x7f, 0xe),
            c => (c, 0xe0, 0x7e),
        };
        if id < 0 {
            return Ok(());
        }
        self.sfx3(id, pos, vol, prio)
    }
}
