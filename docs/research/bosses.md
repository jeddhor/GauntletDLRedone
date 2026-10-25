# Boss dossiers (M15)

Revision SLUS-20047. Native implementation: the critter engine
(research/critters.md: CritterBossAI, moves and patterns, BOSSGEN,
CritterDamage including breakable collision nodes, BossDeath, coin spew),
the quest-item machine (game/boss.rs: QuestItemState 1 → 2 at the boss's
critical move, 3 after 1–3 s, 4 on the legend item's hit, 5, 6 when the item
expires after 29 s for 0x24/0x26/0x27/0x28/0x2a, 99 once the boss is dead),
the good wizard (goodwiz.rs) and the auto exit to TowerWave (0xd00).

Phase rule seen in every boss: without the realm's legend item
(QuestItemState 0 at the level start; GauntWorldInit sets 1 when a player
holds it), the boss's health stops at its critical value (B6's dragon at
104 of 2000) and the fight cannot end. Legend items are collected in the
realm's levels (their movies: A2 iceaxe, B5 javelin, C3 bellows, D4
scimitar, E1 soulsave, G3 fscroll, I4 lamp, J2 lantern, K4 goodbook).
Skorne 2 (F2) and Garm (H4) take no realm legend item (BossType ≥ 0x2b).

Evidence:
- Reference: every boss compared update by update from before activation
  through its first attacks, 250/250 in every part, camera included
  (gameplay.md "The other boss levels"); B6's death, coin spew, good wizard
  and auto exit compared through game_main's level end (warped health).
- Native, from the level start (traversal bot, game_run --bot, legend item
  held as a labelled campaign precondition, health assists counted; batch
  b5, scratch reports):

| Level | Boss (BossType) | Health | QuestItemState seen | Boss dead (update) | Level end → wave |
|---|---|---|---|---|---|
| A5 | chimera, three heads (0x23) | 4500 | 1, 2, 3, 5, 6, 99 | 11550 | 12423 → 0xd00 |
| B6 | dragon (0x22) | 2000 | 1, 2, 5, 99 | 3225 | 4099 → 0xd00 |
| C5 | djinn (0x24) | 3000 | 1, 2, 5, 6 | — | not reached |
| D5 | drider (0x25) | 3250 | 1, 2, 3, 5, 99 | 6975 | 7865 → 0xd00 |
| E2 | Skorne 1 (0x2a) | 6000 | 1, 2, 3, 5, 6, 99 | 32985 (60,000-update run) | 34236 → 0xd00 |
| F2 | Skorne 2 (0x2b) | 3000 | 0, 99 | 20220 | 21093 → 0xd00 (the "garm" movie follows in the client) |
| G5 | lich (0x29) | 4500 | 1, 2, 3, 5, 99 | 5805 | 6686 → 0xd00 |
| H4 | Garm (0x2c) | 5625 | 0, 99 | 13605 | 14310 → 0xd00 ("victory", final stats) |
| I5 | yeti (0x27) | 5000 | 1 | — | defeated from the reference's warp point (below) |
| J5 | wraith (0x28) | 5000 | 1, 2, 3, 5, 6, 99 | 13230 | 14120 → 0xd00 |
| K5 | pboss (0x26) | 3750 | 1, 2, 3, 5, 6, 99 | 5205 | 6079 → 0xd00 |

Nine of eleven bosses are defeated by native play from the level start
(E2 in a 60,000-update run); C5's djinn was defeated in one bot build (b7)
but stalls near 1,556 health in the current one, and the bot does not reach
I5's yeti. 2026-10-23 batch t17 (60,000 updates, --pexp 30, legend item
held): all ten reachable bosses end through the auto exit — K5 5113, B6
3411, C5 8316 (the djinn floats over a pit 17.4 units from the nearest
floor; the bot's boss range now extends to that distance + 4), D5 6352, F2
12250, J5 11803, E2 47389, G5 5422, H4 35892, A5 19121 (level end
updates). I5's yeti still not reached (its arena is a separate floor region; the reference capture of
I5 also needed a warp). Co-op: B6 with 2–4 players (playtest.md
"Co-op matrix"). Not done: a campaign that earns the legend items in play
and chains realm → boss (the full critical path, M15).

I5 from the warp point (2026-10-23): the reference capture of I5 needed a
warp (w9l5-boss-v2: player 0 at (9.6, −4.047, −5.2), 62 units from the
yeti); the native traversal bot started there (game-run DEBUG teleport,
legend item held, --pexp 30, health assists 49) fights the yeti from 5000
to 0, BossIsDead follows and the auto exit ends the level at update
21674 to TowerWave 0xd00 (private/reports/game-run-I5-warp-bot-v1.json).
An idle player at that point never creates the boss instance (activation
needs the approach). With this every boss level's fight and end have run
natively; I5's on-foot route to the arena is still not shown.

I5 from the level start (2026-10-24): the arena is reached by the raft
(object 111) that trigger 40 (flags 0x50a: only while every player stands
on the raft) sends across in ~180 updates. The pad had stayed culled
because native's culling view took the StdCam record's eye under the boss
camera (items.md "Item culling under the boss and trigger cameras"); with
the window's eye the traversal bot (legend item held, --pexp 30, labelled
preconditions as above; 64 health assists) touches the pad, rides the
raft, fights the yeti from 5000 to 0 and the auto exit ends the level at
update 30812 → 0xd00 (private/reports/game-run-I5-bot-start-v1.json). All
eleven boss levels now end from their own start.


## Boss deaths against the reference (2026-10-24)

Every boss's death is now compared update by update with the reference.
Captures: the boss level's start capture with the boss critter's health
written to 10 (reference_warp.py --critter-health 0 10; WARPED, a guest
write), for D5, J5 and A5 after an approach saved by reference_warp (D5:
Up+Right 340 vblanks; J5: Up 360; A5: trace-A5-boss-v2's inputs, 500), I5
from the warped w9l5-boss-v2. Traces: walk, six throw bursts, idle
(trace-<code>-bosskill-v1/-v2, 488–500 vblanks; the first throws kill).
Replays: whole update (game_run --enemy-replay --frame-replay), every part
compared — items, players, effects, world animation, critters, camera,
good wizard.

| Boss | Death (trace step) | Replay | Result |
|---|---|---|---|
| G5 lich | 261 | replay-frame-G5kill-v1 | 250/250, every part |
| C5 djinn | 225 | replay-frame-C5kill-v2 | 250/250 (v1: effects 249 — AddExp's level-up effect, now ported) |
| E2 Skorne 1 | 226 | replay-frame-E2kill-v1, then w5l2-bosskill-v2..v7: -E2wiz2-v1, -E2wiz3-v1, -E2wiz4567-v1 (v4..v7 joined: the wizard 4 → 11 and game_main's level end, mode 0x4012, at step 1700) | 250/250, 250/250, 250/250, 849/849: every part |
| F2 Skorne 2 | 196 | replay-frame-F2kill-v1, then w6l2-bosskill-v2..v6: -F2wiz2-v1, -F2wiz3456-v1 (v3..v6 joined: the wizard 2 → 11 and mode 0x400e — the "garm" movie — at step 1580) | 250/250, 250/250, 789/789: every part |
| H4 Garm | 251 | replay-frame-H4kill-v1, then the chained captures w8l4-bosskill-v2..v5: -H4wiz2-v1 (death move), -H4wiz3-v1 (BossDeath's good wizard 0 → 2), -H4wiz45-v1 (trace-H4-goodwiz-v4 and v5 joined at their link: the wizard 2 → 11, auto_exit_flag, game_main's mode 0x400e — init_gamemovie's "victory" — at step 905) | 250/250, 250/250, 250/250, 452/452: every part, the good wizard included |
| K5 pboss | 220 | replay-frame-K5kill-v1 | 250/250 |
| I5 yeti | 246 | replay-frame-I5kill-v2 | 250/250 |
| J5 wraith | 88 | replay-frame-J5kill-v2 | 244/244 |
| D5 drider | 179 | replay-frame-D5kill-v2 | 250/250 but critters 249: step 185, collision node 1's rotation (+0x5b0..+0x5d8) differs in the third decimal for one update, six updates into the death move |
| A5 chimera | 107 | replay-frame-A5kill-v3 | 244/244 but effects 240: every 15 updates an effect's node starts 0.025 units off (the dead chimera's node pose); v2's coins fixed (below) |
| B6 dragon | 203 | replay-frame-B6kill (2026-10-22) | 250/250, and the good wizard and auto exit after it |

The joined trace: v5 alone starts with the wizard's speech already queued
in v4, and the replay carries native's own EE voice queue (it is not in the
trace), so state 9 (AudioStillPlaying) ended at once; from v4 the speech is
queued natively and runs its 76 vblanks as in the reference.

K2's General (a statue creature, kind 8, riding the ship's moving floor
beside the player from the level start): w11l1-genkill-v1 (WARPED: its
health written to 10 in w11l1-newgame-v2), trace-K2-genkill-v2 (four
Left-and-throw bursts): hits at steps 81 and 105, death at 113, the instance
cleared by 205; replay-frame-K2genkill-v1 260/260 in every part.

A1's golem (statue creature, kind 3): w1l1-golem-v3 (trace-A1-golem-v2's
walk until after the spawn), w1l1-golemkill-v1 (WARPED: health 10),
trace-A1-golemkill-v2 (walk Down, throw): hits at 198 and 224 (death), the
instance cleared by 312 with CritterDropItem's bag; replay-frame-A1golemkill
240/240 in every part after StartBagFX took SfxSetPhysics (the bag's node
yaw from its velocity; v1 had effects 239/240).

A1's gargoyle (kind 7): w1l1-statue-v3 (trace-A1-statue-v2's inputs past
the awakening), w1l1-gargkill-v1 (WARPED: health 10), trace-A1-gargkill-v1
(Down-Left and throw bursts): hits at 44, 122, 158, death at 166, cleared by
220; replay-frame-A1gargkill 206/206 in every part. The Death statue stays
unreached (fenced, gameplay.md "Statues").

Fixed on the way: AddExp's StartLevelUpFX (a level up in play started no
level-up effect natively; the Tower had it), and ProcessSpewItems'
UpdateObjWorldMat (coins spewed onto A5's parented arena floor went back to
their spawn point at the next GetObjWorldMat). Open residuals: D5's
collision-node rotation and A5's effect node position, both from the dead
boss's animated node pose (first divergent observations above).

## The critical path (2026-10-24)

From the ported gates (tower.rs: WorldOpen 0x177778, HideExits 0x15b690;
critter_ai.rs BossDeath; pickups.rs PlayerGiveRune):
- An ordinary realm opens when a player holds all of its WorldCrystal set;
  inside a realm, level n needs bit n−1 of that world's wave byte, so its
  levels are played in order.
- Each boss death gives the players in state 1 the shard bit of the realm's
  place in WorldOrder (13, 7, 2, 1, 11, 4, 3, 9, 10, 5, 6, 8): G 1, B 2, A 3,
  K 4, D 5, C 6, I 7, J 8, E 9, F 10, H 11.
- The Temple (E) opens with shards 0x1fe — the eight ordinary bosses; Hell
  (F) with 0x3fe (E2's too) and runes 0xfff; Battle (H) with 0x7fe (F2's
  too); H4 (Garm, the ending) with all thirteen runes 0x1fff.
- Rune stones (kind 1 subtype 10, one per level, PlayerGiveRune to every
  player) lie in A3 B1 C1 D2 G1 G4 H3 I2 I3 J4 J6 K1 K3; legend items
  (subtype 0xd) in A2 B5 C3 D4 E1 G3 I4 J2 K4 (item survey of all 61
  levels' loads, game-run treasures).
So the ending needs every realm's levels in order, each boss with its
legend item, and all thirteen runes before H4: in effect every non-secret
level. The traversal tools seek runes and legend items before exits and
campaign_chain.py --require makes a level count only with them taken.
