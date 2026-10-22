//! The healing side of high-level magic (damage flag 0x800000, set by
//! start_magic for players past level 0x18): do_heal_players 0x132620 (a
//! drained enemy or critter heals the caster and nearby players), HealItem
//! 0x15b010 (ProcessEffects: the blast transforms items), StartHealFx and
//! ReplaceItemAtree's tree swaps (`Game::item_trees`, presentation).
use super::{Game, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

type V3 = [f32; 3];

impl Game {
    /// StartHealFx(matrix, header): StartFXNoLoop at the translation (the
    /// matrix copy is presentation); −1 starts nothing.
    pub(crate) fn start_heal_fx(&mut self, at: V3, header: i32) -> R<()> {
        if 0 <= header {
            self.start_fx_no_loop(header as usize, at)?;
        }
        Ok(())
    }

    /// ReplaceItemAtree(item, package, tree): the item draws `tree` from
    /// now on (until the slot is deleted or reused).
    pub(crate) fn replace_item_atree(&mut self, slot: usize, tree: &str) {
        self.item_trees.insert(slot, tree.to_string());
    }

    /// do_heal_players(amount, player, matrix): past level 0x4a the caster
    /// heals amount × ((level − 0x4b) × 0.016 + 0.1), every other player in
    /// state 1 whose x/z distance is below the caster's +0x120 half that;
    /// NewHelp 0x93 and StartHealFx(matrix, 0x32).
    pub(crate) fn do_heal_players(&mut self, amount: f32, p: usize, at: V3) -> R<()> {
        let level = self.players[p].i32_(0x2ac0);
        if level <= 0x4a {
            return Ok(());
        }
        let f = add(mul((level - 0x4b) as f32, f32::from_bits(0x3c83_126f))?, f32::from_bits(0x3dcc_cccd))?;
        self.heal_player(mul(amount, f)?, p)?;
        let me = self.players[p].u32_(0) as usize;
        for k in 0..4 {
            if k == me || self.players[k].i32_(0xfc) != 1 {
                continue;
            }
            let (a, b) = (self.players[p].v3(0x50), self.players[k].v3(0x50));
            let d = crate::vu0::fqdist(sub(a[0], b[0])?, sub(a[2], b[2])?)?.0;
            if d < self.players[p].f32_(0x120) {
                self.heal_player(mul(mul(amount, f)?, 0.5)?, k)?;
            }
        }
        let pos = self.players[p].v3(0x50);
        self.help_at(0x93, p, Some(pos));
        self.start_heal_fx(at, 0x32)
    }

    /// HealItem(item, player): by the caster's class bits (+8 & 3) and level
    /// it upgrades gold (piles and chests: silver at 25, gold at 50 for
    /// bits 0), restores spoiled food (apple at 25, chicken at 50 for bits
    /// 2), revives kind-8 items (bits 1) and kind-10 items (bits 3; at 50
    /// it destroys them with DamageItem(10000)); heal effect and help.
    pub(crate) fn heal_item(&mut self, slot: usize, p: usize) -> R<()> {
        let level = self.players[p].i32_(0x2ac0);
        let bits = self.players[p].u32_(8) & 3;
        let me = self.players[p].u32_(0) as i32;
        let mut fx = -1;
        let mut help = -1;
        let r = self.items.records[slot].clone();
        let def = r.definition();
        let kind = self.items.definitions[def].kind();
        let sub_ = self.items.definitions[def].subtype();
        let find = |g: &Self, kind: i32, st: i32, name: &[u8]| -> R<i16> {
            g.items.find_info_index(kind, st, name).map(|i| i as i16).ok_or_else(|| super::Unsupported(format!("item {}", String::from_utf8_lossy(name))))
        };
        match kind {
            2 => 'chest: {
                let content = r.i16_(0xf0);
                let Ok(c) = usize::try_from(content) else { break 'chest };
                let cd = &self.items.definitions[c];
                if cd.kind() != 1 {
                    break 'chest;
                }
                let (cs, cv) = (cd.subtype(), cd.i16_(0x40));
                if cs == 1 {
                    if bits != 0 || 10 < cv || level < 0x19 {
                        break 'chest;
                    }
                    let (tree, amount, name, h) = if level < 0x32 { ("CHESTG1", 100, &b"TREAS_SILVER"[..], 0x8b) } else { ("CHESTG3", 200, &b"TREAS_GOLD"[..], 0x8c) };
                    if sub_ != 0x2b {
                        self.replace_item_atree(slot, tree);
                    }
                    if 0x32 <= level {
                        self.items.records[slot].set_i16(0x100, amount);
                    }
                    if r.i8_(0xdc) != 0 {
                        self.items.records[slot].set_i32(0xf4, i32::from(amount));
                    } else {
                        if level < 0x32 {
                            self.items.records[slot].set_i16(0x100, amount);
                        }
                        let n = find(self, 1, 1, name)?;
                        self.items.records[slot].set_i16(0xf0, n);
                    }
                    (fx, help) = (0x30, h);
                } else if cs == 3 && r.i8_(0xdc) <= 0 && bits == 2 {
                    if -100 < cv {
                        if 0 <= cv || level < 0x19 {
                            break 'chest;
                        }
                        let n = find(self, 1, 3, b"APPLE")?;
                        self.items.records[slot].set_i16(0xf0, n);
                        (fx, help) = (0x2f, 0x8f);
                    } else {
                        if level < 0x32 {
                            break 'chest;
                        }
                        let n = find(self, 1, 3, b"CHICKEN")?;
                        self.items.records[slot].set_i16(0xf0, n);
                        (fx, help) = (0x2f, 0x90);
                    }
                }
            }
            8 if bits == 1 => {
                if level < 0x32 {
                    if r.i16_(0xda) < 0x21c {
                        fx = 0x31;
                    }
                    let rr = &mut self.items.records[slot];
                    rr.set_u8(0xdc, 0);
                    rr.set_u8(0xde, 0);
                    rr.set_i16(0xda, 600);
                    // AnimateATree(tree, 0, 3): presentation.
                    help = 0x8d;
                } else if r.i16_(0xd8) != 0 {
                    // "<name>_D" (AtreeMatchAnyHeader); without it the item goes.
                    let name = format!("{}_D", String::from_utf8_lossy(self.items.definitions[def].name()));
                    if self.effects.tables.shared_header(&name).is_some() {
                        self.replace_item_atree(slot, &name);
                        let rr = &mut self.items.records[slot];
                        rr.set_u8(0xdc, 0);
                        rr.set_u8(0xde, 0);
                        rr.set_i16(0xd8, 0);
                    } else {
                        self.delete_item(slot, false);
                    }
                    (fx, help) = (0x31, 0x8e);
                }
            }
            10 if !matches!(sub_, 0x29 | 0x2b) && bits == 3 => {
                if level < 0x32 {
                    self.items.records[slot].set_i16(0xf4, 4);
                    help = 0x91;
                } else {
                    self.damage_item(10000.0, slot, 0, me)?;
                    help = 0x92;
                }
            }
            1 => {
                let v = r.i32_(0xf4);
                if sub_ == 1 {
                    if bits == 0 && v <= 10 && 0x19 <= level {
                        let (tree, amount, h) = if level < 0x32 { ("TREAS_SILVER", 100, 0x8b) } else { ("TREAS_GOLD", 200, 0x8c) };
                        self.replace_item_atree(slot, tree);
                        self.items.records[slot].set_i32(0xf4, amount);
                        (fx, help) = (0x30, h);
                    }
                } else if sub_ == 3 && bits == 2 {
                    if -100 < v {
                        if v < 0 && 0x19 <= level {
                            self.replace_item_atree(slot, "APPLE");
                            self.items.records[slot].set_i32(0xf4, 0x32);
                            (fx, help) = (0x2f, 0x8f);
                        }
                    } else if 0x32 <= level {
                        self.replace_item_atree(slot, "CHICKEN");
                        self.items.records[slot].set_i32(0xf4, 100);
                        (fx, help) = (0x2f, 0x90);
                    }
                }
            }
            _ => {}
        }
        let at = r.v3(0x40);
        self.start_heal_fx(at, fx)?;
        if 0 <= help {
            let pos = self.players[p].v3(0x50);
            self.help_at(help, p, Some(pos));
        }
        Ok(())
    }
}
