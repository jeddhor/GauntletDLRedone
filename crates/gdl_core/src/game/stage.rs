//! The treasure-room round trip: SaveStage 0x1914b8 (a secret exit, item
//! type 9 subtype 0x32, before leaving for world 12) and LoadStage 0x1916a0
//! (init_next_level when skip_wave is 13..0xffff, i.e. the return to
//! recorded_wave). The state crosses the level load through the host
//! (`Game::stage_save`), as the original keeps it in globals.
use super::{Game, R};
use serde::{Deserialize, Serialize};

type V3 = [f32; 3];

/// SaveStage's globals: recorded_wave, secret_pos, secret_leader, the item
/// status/type arrays (itemstatus/itemtype, ≤ 0x400) and the item world
/// objects' trigger bytes (wobjtrg/wobjptrg, ≤ 0x96). The camera records
/// (SaveAllRecords: CopyCam, posbuf, transmitters) are not kept: the
/// return re-initialises the camera settled on the player (HYPOTHESIS).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StageSave {
    pub recorded_wave: i32,
    pub secret_pos: V3,
    pub secret_leader: usize,
    /// (+0xd8 status, definition kind); −1 marks an absent or excluded item.
    pub items: Vec<(i16, i32)>,
    pub wobjs: Vec<(u8, u8)>,
}

impl Game {
    /// SaveStage(item, player, position): the exit item itself, empty slots
    /// and open chests (kind 2 with +0xdc > 0) are saved as gone.
    pub(crate) fn save_stage(&mut self, exit: usize, leader: usize, at: V3) {
        let n = self.items.records.len().min(0x400);
        let items = (0..n)
            .map(|i| {
                let r = &self.items.records[i];
                let kind = self.items.definitions.get(r.definition()).map_or(0, |d| d.kind());
                if i == exit || !r.active() || (kind == 2 && 0 < r.i8_(0xdc)) {
                    (-1, 1)
                } else {
                    (r.i16_(0xd8), kind)
                }
            })
            .collect();
        let wobjs = self
            .items
            .wobjs
            .iter()
            .take(0x96)
            .map(|w| self.items.objects.get(w.object).map_or((0, 0), |o| (o.byte_16, o.byte_17)))
            .collect();
        let wave = self.globals.cur_world << 8 | (self.globals.cur_level & 0xff);
        self.stage_save = Some(StageSave { recorded_wave: wave, secret_pos: at, secret_leader: leader, items, wobjs });
    }

    /// LoadStage: the first player in state 1 at secret_pos (others are
    /// placed by the host beside it), every saved item's status back (a
    /// changed type or a spent generator — kind 3 without +0xd8 bit 0 — is
    /// gone), the world objects' trigger bytes back, then the camera settled.
    pub fn load_stage(&mut self, s: &StageSave) -> R<()> {
        if let Some(p) = (0..4).find(|&k| self.players[k].i32_(0xfc) == 1) {
            let r = &mut self.players[p];
            r.set_f32(0x50, s.secret_pos[0]);
            r.set_f32(0x54, s.secret_pos[1]);
            r.set_f32(0x58, s.secret_pos[2]);
            self.update_player_world_mat(p, true)?;
        }
        for i in 0..self.items.records.len() {
            let kind = self.items.definitions.get(self.items.records[i].definition()).map_or(0, |d| d.kind());
            let saved = s.items.get(i).copied().unwrap_or((-1, 1));
            let r = &mut self.items.records[i];
            if !r.active() || saved.1 != kind {
                r.set_i16(0xd8, -1);
                continue;
            }
            r.set_i16(0xd8, saved.0);
            if kind == 3 && r.u16_(0xd8) & 1 == 0 {
                r.set_u8(0xe3, 0xff);
                r.set_u8(0xf6, 0);
                r.set_i16(0xd8, -1);
            }
        }
        for (k, w) in self.items.wobjs.clone().iter().enumerate() {
            if let (Some(&(a, b)), Some(o)) = (s.wobjs.get(k), self.items.objects.get_mut(w.object)) {
                o.byte_16 = a;
                o.byte_17 = b;
            }
        }
        self.start_camera_settled()
    }
}
