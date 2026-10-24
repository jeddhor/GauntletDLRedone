#!/bin/bash
# Replay regression over the preserved reference traces (M10-M15 evidence).
#
#   bash tools/regress.sh TAG
#
# Every replay is a new report private/reports/replay-<name>-TAG.json
# (game_run.py refuses existing reports); one summary line per replay goes
# to stdout ("<name> {...} items {...}", the format coverage_matrix.py
# reads). GDL_TOOLS selects the gdl_tools binary (game_run.py); use a copy
# that no build replaces while the run lasts. The traces are actual-disc
# reference traces; the warped ones are labelled in their own source.
set -e
cd "$(dirname "$0")/.."
R=private/reports
SEED="--items-seed $R/live-items-g1-fixture-v1.json"
G="--world G --level 1"
V=$1
[ -n "$V" ] || { echo "usage: tools/regress.sh TAG" >&2; exit 2; }
run() { python3 tools/python/game_run.py "$@" > /dev/null 2>&1 || true; }
run $R/replay-frame-fight-g1-$V.json $G --enemy-replay $R/trace-fight-g1-v6.json $SEED --frame-replay
run $R/replay-frame-combat-g1-$V.json $G --enemy-replay $R/trace-combat-g1-v4.json $SEED --frame-replay
run $R/replay-frame-varied-g1-$V.json $G --enemy-replay $R/trace-varied-g1-v2.json $SEED --frame-replay
run $R/replay-frame-tower-$V.json --world L --level 1 --enemy-replay $R/trace-tower-v1.json --frame-replay --load-players 0
run $R/replay-enemies-g1-$V.json $G --enemy-replay $R/trace-enemies-g1-v4.json $SEED
run $R/replay-enemies-fight-g1-$V.json $G --enemy-replay $R/trace-fight-g1-v6.json $SEED
run $R/replay-player-g1-$V.json $G --replay $R/trace-player-g1-v2.json
run $R/replay-player-combat-g1-$V.json $G --replay $R/trace-combat-g1-v3.json
run $R/replay-player-fight-g1-$V.json $G --replay $R/trace-fight-g1-v6.json
run $R/replay-frame-a1-$V.json --world A --level 1 --enemy-replay $R/trace-a1-newgame-v2.json --frame-replay
run $R/replay-frame-b1-$V.json --world B --level 1 --enemy-replay $R/trace-b1-newgame-v2.json --frame-replay
run $R/replay-frame-k2-$V.json --world K --level 2 --enemy-replay $R/trace-k2-newgame-v5.json --frame-replay
run $R/replay-frame-g1new-$V.json $G --enemy-replay $R/trace-g1-newgame-v4.json --frame-replay
run $R/replay-frame-b6-$V.json --world B --level 6 --enemy-replay $R/trace-b6-boss-v4.json --frame-replay
run $R/replay-frame-g5-$V.json --world G --level 5 --enemy-replay $R/trace-g5-boss-v1.json --frame-replay
run $R/replay-frame-A1statue-$V.json --world A --level 1 --enemy-replay $R/trace-A1-statue-v2.json --frame-replay
run $R/replay-frame-B6kill-$V.json --world B --level 6 --enemy-replay $R/trace-B6-bosskill-v1.json --frame-replay
for g in 2 3 4; do run $R/replay-frame-B6wiz$g-$V.json --world B --level 6 --enemy-replay $R/trace-B6-goodwiz-v$g.json --frame-replay; done
run $R/replay-frame-A1golem-$V.json --world A --level 1 --enemy-replay $R/trace-A1-golem-v2.json --frame-replay
run $R/replay-frame-A1death-$V.json --world A --level 1 --enemy-replay $R/trace-A1-death-v1.json --frame-replay
run $R/replay-frame-G1exit-$V.json --world G --level 1 --enemy-replay $R/trace-G1-exit-v1.json --frame-replay
run $R/replay-frame-C1water-$V.json --world C --level 1 --enemy-replay $R/trace-C1-water-v1 --frame-replay
run $R/replay-frame-C1water2-$V.json --world C --level 1 --enemy-replay $R/trace-C1-water-v2 --frame-replay
run $R/replay-frame-G1potion-$V.json --world G --level 1 --enemy-replay $R/trace-G1-potion-v2 --frame-replay
run $R/replay-frame-G1xbow-$V.json --world G --level 1 --enemy-replay $R/trace-G1-xbow-v1 --frame-replay
run $R/replay-frame-A1trans-$V.json --world A --level 1 --enemy-replay $R/trace-A1-trans-v1 --frame-replay
for b in C5 D5 I5 J5 K5 E2 F2 H4 A5; do
  t=$R/trace-$b-boss-v1.json; [ $b = E2 ] && t=$R/trace-E2-boss-v2.json; [ $b = A5 ] && t=$R/trace-A5-boss-v2.json; [ $b = D5 ] && t=$R/trace-D5-boss-v2.json; [ $b = I5 ] && t=$R/trace-I5-boss-v2.json
  run $R/replay-frame-$b-$V.json --world ${b:0:1} --level ${b:1} --enemy-replay $t --frame-replay
done
# J1's v2 re-records the same capture with RandomItemNum (the chest's random content).
for L in D E F H I J; do t=$R/trace-${L}1-newgame-v1; [ $L = J ] && t=$R/trace-J1-newgame-v2
  run $R/replay-frame-${L}1new-$V.json --world $L --level 1 --enemy-replay $t --frame-replay; done
# D3 (WARPED captures w4l3-start-v1 / w4l3-leaf-v1): the level start and the falling leaves (kind 10 subtype 0x31).
run $R/replay-frame-D3start-$V.json --world D --level 3 --enemy-replay $R/trace-D3-start-v1 --frame-replay
run $R/replay-frame-D3leaf-$V.json --world D --level 3 --enemy-replay $R/trace-D3-leaf-v1 --frame-replay
# I2 and B2 (WARPED w9l2-start-v1 / w2l2-start-v1): level starts; B2 starts with
# a stale Cur3DTransmitter (57, the Tower's) past its 42 transmitters.
run $R/replay-frame-I2start-$V.json --world I --level 2 --enemy-replay $R/trace-I2-start-v1 --frame-replay
run $R/replay-frame-B2start-$V.json --world B --level 2 --enemy-replay $R/trace-B2-start-v1 --frame-replay
# The level-start sweep (2026-10-23): every level without another reference
# replay, warped from slot 3 by do_exit's wave skip (WARPED captures
# lvl-<L>-start-v1, w10l4-start-v1 for J4, w10l3-start-v1 for J6), idle.
SWEEP="A2 A3 A4 A6 B3 B4 B5 C2 C3 C4 D2 D4 G2 G3 G4 H2 H3 I3 I4 J2 J3 J4 J6 K1 K3 K4 S1 S2 S3 S4 S5 S6 S7 S8 S9"
# J4's v2 (and B5's combat v2) re-record the same captures with the item_wobjs and CheckNum probes.
for L in $SWEEP; do t=$R/trace-$L-start-v1; [ $L = J4 ] && t=$R/trace-J4-start-v2
  run $R/replay-frame-${L}start-$V.json --world ${L:0:1} --level ${L:1} --enemy-replay $t --frame-replay; done
# The combat sweep (2026-10-23 night): from lvl-<L>-play-v1 (the start
# capture 210 vblanks on), 600 vblanks of attacks while turning.
# K4's and J4's v2 add the enemy node parents (enemies riding moving floors).
for L in $SWEEP; do t=$R/trace-$L-combat-v1; case $L in B5|J4|K4) t=$R/trace-$L-combat-v2;; esac
  run $R/replay-frame-${L}combat-$V.json --world ${L:0:1} --level ${L:1} --enemy-replay $t --frame-replay; done
# Two players at B6's boss (WARPED w2l6-2p-v1: tower-2p-v2 warped by the wave skip, 500 vblanks of both players' moves and attacks).
run $R/replay-frame-B6two-$V.json --world B --level 6 --enemy-replay $R/trace-B6-2p-v1 --frame-replay --vplayers 2
for f in frame-fight-g1 frame-combat-g1 frame-varied-g1 frame-tower enemies-g1 enemies-fight-g1 player-g1 player-combat-g1 player-fight-g1 frame-a1 frame-b1 frame-k2 frame-g1new frame-b6 frame-g5 frame-C5 frame-D5 frame-I5 frame-J5 frame-K5 frame-E2 frame-F2 frame-H4 frame-A5 frame-A1statue frame-A1golem frame-A1death frame-B6kill frame-B6wiz2 frame-B6wiz3 frame-B6wiz4 frame-G1exit frame-C1water frame-C1water2 frame-G1potion frame-G1xbow frame-A1trans frame-D1new frame-E1new frame-F1new frame-H1new frame-I1new frame-J1new frame-D3start frame-D3leaf frame-I2start frame-B2start $(for L in $SWEEP; do echo frame-${L}start; done) $(for L in $SWEEP; do echo frame-${L}combat; done) frame-B6two; do
python3 -c "
import json, pathlib
p = pathlib.Path('$R/replay-$f-$V.json')
if not p.exists():
    print('$f', 'MISSING')
else:
    r = json.loads(p.read_text())
    print('$f', {k: r.get(k) for k in ('updates', 'equal_updates', 'item_updates_equal', 'player_updates_equal', 'effect_updates_equal', 'world_anim_updates_equal', 'critter_updates_equal', 'camera_updates_equal', 'wizard_updates_equal') if k in r}, 'items', r.get('differing_item_slots'))
"
done
