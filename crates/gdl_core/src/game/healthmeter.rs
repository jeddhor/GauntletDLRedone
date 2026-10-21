//! Boss health meters: HealthMeterInit 0x19e018, HealthMeterStart
//! 0x19e038, HealthMeterUpdate 0x19e290, CritterAddHealthMeter and
//! CritterUpdateHealthMeter. A critter type with +0x5c bit 4 gets a meter
//! (at most 3): +0xf8 pieces of 256 screen units, +0xfa width (advances the
//! next meter's x when +0x5c bit 8 also adds the backgrounds), +0xfc/+0xfe
//! the left/right insets; blits "<TYPE>_METER_BG<n>" / "_FG<n>" at
//! (x + 256·(n − 1), 0) with alpha 0x70. The shown value follows the
//! critter's health (+0x4f4) by 3 × vb_elapsed per frame. The in-world
//! meter tree (+0x5c bit 0x800, HEALTHMETER node scaled by health) is not
//! drawn.
use super::hud::Blit;
use super::Game;

#[derive(Clone, Debug)]
pub struct HealthMeter {
    pub name: String,
    pub pieces: i32,
    pub x: i32,
    pub xl: i32,
    pub xr: i32,
    pub max: f32,
    pub value: f32,
    pub with_bg: bool,
}

/// HealthMeterNum / HealthMeterX / HealthMeterNextX and the meters.
#[derive(Clone, Debug, Default)]
pub struct HealthMeters {
    pub meters: Vec<HealthMeter>,
    pub x: i32,
    pub next_x: i32,
}

impl Game {
    /// CritterAddHealthMeter(i): HealthMeterStart for a type with +0x5c
    /// bit 4; the index (or −1) at +0x484.
    pub(crate) fn critter_add_health_meter(&mut self, i: usize) {
        let ty = self.critter_type_rec(i).to_vec();
        let u32_ = |o: usize| ty.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let i16_ = |o: usize| ty.get(o..o + 2).map_or(0, |b| i16::from_le_bytes(b.try_into().unwrap()));
        let flags = u32_(0x5c);
        let mut idx = -1i16;
        if flags & 4 != 0 {
            let name: String = ty.iter().take(16).take_while(|c| **c != 0).map(|c| char::from(*c)).collect();
            let max = self.crit(i).rec.f32_(0x4f4);
            let m = &mut self.health_meters;
            if m.meters.len() < 3 {
                let with_bg = flags >> 3 & 1 != 0;
                if with_bg {
                    m.x = m.next_x;
                    m.next_x += i32::from(i16_(0xfa));
                }
                idx = m.meters.len() as i16;
                m.meters.push(HealthMeter { name, pieces: i32::from(i16_(0xf8)), x: m.x, xl: i32::from(i16_(0xfc)), xr: i32::from(i16_(0xfe)), max, value: max, with_bg });
            }
        }
        self.crit_mut(i).rec.set_i16(0x484, idx);
    }

    /// CritterUpdateHealthMeter(i): HealthMeterUpdate(health) for its meter.
    pub(crate) fn critter_update_health_meter(&mut self, i: usize) {
        let idx = self.crit(i).rec.i16_(0x484);
        let Ok(k) = usize::try_from(idx) else { return };
        let target = self.crit(i).rec.f32_(0x4f4).max(0.0);
        let step = (self.globals.vb_elapsed * 3) as f32;
        let Some(m) = self.health_meters.meters.get_mut(k) else { return };
        if target < m.value {
            m.value = (m.value - step).max(target);
        } else if m.value < target {
            m.value = (m.value + step).min(target);
        }
    }

    /// The meters' blits for this frame (HealthMeterUpdate's sizes and
    /// crops; the backgrounds tinted 0x8080ff while the boss's +0xb4c
    /// quest-item time runs).
    pub fn health_meter_blits(&self) -> Vec<Blit> {
        let mut out = Vec::new();
        let frozen = usize::try_from(self.boss_crit_inst - 1).ok().and_then(|b| self.critters.get(b)).and_then(Option::as_ref).is_some_and(|c| 0 < c.rec.i16_(0xb4c));
        for m in &self.health_meters.meters {
            let sep = if m.name.is_empty() { "" } else { "_" };
            let frac = if 0.0 < m.max { m.value / m.max } else { 0.0 };
            for n in 0..m.pieces.max(0) {
                let x = m.x + 0x100 * n;
                if m.with_bg {
                    let mut b = self.hud.make_pub(self.hud.lookup(&format!("{}{sep}METER_BG{}", m.name, n + 1)), x, 0, -1, -1);
                    b.rgba = super::hud::pack_rgb_pub(if frozen { 0x8080ff } else { 0xffffff }, (0x80 - (0x70 >> 1)) as u8);
                    out.push(b);
                }
                // The foreground's width (and crop) by the shown fraction.
                let w = if m.pieces == 2 {
                    let f = frac * 2.0;
                    if n == 0 {
                        if f < 1.0 { Some((m.xl as f32 + f * (0x100 - m.xl) as f32) as i32) } else { None }
                    } else if 0.0 < f - 1.0 {
                        Some(((f - 1.0) * (0x100 - m.xr) as f32) as i32)
                    } else {
                        Some(0)
                    }
                } else if 0.0 < frac {
                    Some((frac * (0x100 - (m.xl + m.xr)) as f32) as i32)
                } else {
                    Some(0)
                };
                let mut b = self.hud.make_pub(self.hud.lookup(&format!("{}{sep}METER_FG{}", m.name, n + 1)), x, 0, -1, -1);
                b.rgba[3] = (0x80 - (0x70 >> 1)) as u8;
                if let Some(w) = w {
                    b.w = w as f32;
                    b.crop[1] = w as f32 / 256.0;
                }
                out.push(b);
            }
        }
        out
    }
}
