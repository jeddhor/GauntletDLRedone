//! Scene lighting state: InitLighting 0x180a30, DoLighting 0x180888 (main,
//! once per frame), SetAmbientSpecial 0x180840 and world_update 0x163480's
//! lightning (A5, F2; set up by GauntWorldInit 0x162e38) and end-of-stage
//! intensity fade. The renderer consumes `ambient` (MBSetAmbient) and
//! `lightmap_add` (MBLightmapMod(1, AmbientSpecialCurValue)) as
//! presentation inputs.
use super::{Game, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

/// Ambient 0x249050, GlobalInten 0x24904c, AmbientSpecialTime/Value/CurValue
/// 0x2c34a8.., lightning_wobj/lightning_time (world node, hidden, next
/// time) and WorldEndTime 0x2c2c8c.
#[derive(Clone, Debug, Default)]
pub struct Lighting {
    pub ambient: f32,
    pub global_inten: f32,
    pub special_time: f32,
    pub special_value: f32,
    pub special_cur: f32,
    pub lightning: Vec<Lightning>,
    pub world_end_time: f32,
    /// DoLighting's outputs: MBSetAmbient's value and MBLightmapMod's add.
    pub ambient_out: f32,
    pub lightmap_add: f32,
}

#[derive(Clone, Debug)]
pub struct Lightning {
    pub node: Option<usize>,
    pub hidden: bool,
    pub time: f32,
}

impl Game {
    /// InitLighting(1) for a level: Ambient = LEVL +0xec (the directional
    /// light +0xf0/+0xfc/+0x108 is read by the renderer), GlobalInten 1,
    /// the specials cleared, then DoLighting.
    pub fn init_lighting(&mut self) {
        let ambient = if self.level.len() >= 0xf0 { self.level_f32(0xec) } else { 1.0 };
        self.lighting = super::lighting::Lighting { ambient, global_inten: 1.0, ..Default::default() };
        self.do_lighting();
    }

    /// GauntWorldInit's lightning: A5 (A5XPXLNC_LNING1..4) and F2
    /// (F2XPNC_LNING1..4) world objects start hidden, each next due at
    /// Time + 5 + Random(10).
    pub fn init_lightning(&mut self) -> R<()> {
        let prefix = if self.in_level(b"A5\0\0") {
            "A5XPXLNC_LNING"
        } else if self.in_level(b"F2\0\0") {
            "F2XPNC_LNING"
        } else {
            return Ok(());
        };
        self.lighting.lightning.clear();
        for k in 1..=4 {
            let name = format!("{prefix}{k}");
            let node = self.find_world_object(&name);
            if node.is_some() {
                self.world_node_set_flags(&name, 2);
            }
            let r = self.random_pub(10.0)?;
            let time = add(add(self.globals.time, 5.0)?, r)?;
            self.lighting.lightning.push(Lightning { node, hidden: true, time });
        }
        Ok(())
    }

    /// InLevel 0x1627b8: LEVL +8's first byte equals s[0] and each of +9..+0xb
    /// is NUL or equals s[k].
    pub fn in_level(&self, s: &[u8; 4]) -> bool {
        let Some(c) = self.level.get(8..0xc) else { return false };
        c[0] == s[0] && (1..4).all(|k| c[k] == 0 || c[k] == s[k])
    }

    /// SetAmbientSpecial(duration, value): a later end wins; otherwise the
    /// running special is kept.
    pub fn set_ambient_special(&mut self, duration: f32, value: f32) -> R<()> {
        let end = add(add(duration, self.globals.time)?, f32::from_bits(0x3d08_8889))?;
        let l = &mut self.lighting;
        if l.special_time < end {
            l.special_value = value;
            l.special_time = end;
        }
        Ok(())
    }

    /// world_update's lightning flashes: a shown bolt hides when due (next
    /// Time + 5 + Random(10)); a hidden one flashes (SetAmbientSpecial(0.1,
    /// 0.5)) for 0.2 s.
    pub(crate) fn world_update_lightning(&mut self) -> R<()> {
        for k in 0..self.lighting.lightning.len() {
            let Some(n) = self.lighting.lightning[k].node else { continue };
            let time = self.globals.time;
            if self.lighting.lightning[k].time > time {
                continue;
            }
            let name = self.world_labels[n].clone();
            if !self.lighting.lightning[k].hidden {
                self.world_node_set_flags(&name, 2);
                let r = self.random_pub(10.0)?;
                self.lighting.lightning[k].time = add(add(time, 5.0)?, r)?;
                self.lighting.lightning[k].hidden = true;
            } else {
                self.set_ambient_special(f32::from_bits(0x3dcc_cccd), 0.5)?;
                self.world_node_clear_flags(&name, 2);
                self.lighting.lightning[k].time = add(time, f32::from_bits(0x3e4c_cccd))?;
                self.lighting.lightning[k].hidden = false;
            }
        }
        Ok(())
    }

    /// world_update's end-of-stage block (game mode 0x4010): GlobalInten
    /// falls 1 → 0 over the first second after WorldEndTime (the
    /// TransitionBlit fades out over two), then −1 and end_of_stage 2.
    /// HYPOTHESIS: AudioQueUpdate reports an empty queue.
    pub(crate) fn world_update_end_of_stage(&mut self) -> R<()> {
        if self.globals.game_mode != 0x4010 || self.globals.end_of_stage == 0 {
            return Ok(());
        }
        if self.lighting.world_end_time == 0.0 {
            self.lighting.world_end_time = self.globals.time;
            return Ok(());
        }
        let t = sub(self.globals.time, self.lighting.world_end_time)?;
        if t < 2.0 {
            self.lighting.global_inten = sub(1.0, t)?;
        } else {
            self.lighting.global_inten = -1.0;
            self.globals.end_of_stage = 2;
        }
        Ok(())
    }

    /// DoLighting: the special decays (× 0.6 once its time passed, 0 below
    /// 0.05), the current value follows it by at most +0.05 / −0.25 per call;
    /// ambient = clamp(Ambient · GlobalInten + current, 0, 1).
    pub fn do_lighting(&mut self) {
        let time = self.globals.time;
        let l = &mut self.lighting;
        if l.special_value != 0.0 && l.special_time < time {
            l.special_value = mul(l.special_value, 0.6).unwrap_or(0.0);
            if l.special_value.abs() < 0.05 {
                l.special_value = 0.0;
            }
        }
        if l.special_value != l.special_cur {
            let d = l.special_value - l.special_cur;
            l.special_cur += d.clamp(-0.25, 0.05);
        }
        l.ambient_out = (l.ambient * l.global_inten + l.special_cur).clamp(0.0, 1.0);
        l.lightmap_add = l.special_cur;
    }
}
