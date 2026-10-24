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

