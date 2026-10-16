# Gauntlet: Dark Legacy — native PS2 reconstruction

Implementation follows [the master plan](Gauntlet_Dark_Legacy_PS2_Rust_Bevy_Master_Plan.md).
The port is in development. Current native functionality is a bounded archive
import/extraction tool, original-texture and level inspectors, and a headless
source-qualified animation sampler with a native pose inspector. Gameplay has not
been implemented; the viewer is not a playable port.

See [milestone coverage](docs/coverage.md), [current state](docs/state.md),
[toolchain](docs/toolchain.md) and [disc profile](profiles/ps2/slus-20047.json).
Retail assets, BIOS files, saves and reference captures remain local and ignored.

The session's root .git path is reserved read-only. Source history is kept in
private/source-control.git; use `bash tools/git-local.sh status` and other normal
Git commands through that wrapper. Its commits contain code/docs only.

Build the pinned Rust workspace:

```sh
cargo build --release --locked
cargo test -p gdl_core -p gdl_formats -p gdl_tools --locked
python3 -m unittest discover -s tools/python -p 'test_*.py'
```

The existing local extraction is in private/disc/. To reproduce the disc audit
after a fresh extraction:

```sh
7z x 'Gauntlet - Dark Legacy.iso' -oprivate/disc -y
python3 tools/python/disc_dossier.py 'Gauntlet - Dark Legacy.iso'
```

Inspect or extract WAD.BIN to a **new** output path. Existing outputs are preserved:

```sh
target/release/gdl_tools wad-inspect private/disc/WAD.BIN private/reports/wad-new.json
target/release/gdl_tools wad-extract private/disc/WAD.BIN private/wad-new
```

The established cache is private/wad/; manifests bind each payload to the archive
SHA-256, index and original path hash. Filenames are index/hash IDs, so unknowns
and hash collisions cannot overwrite one another. Native metadata/pixel inspection:

```sh
target/release/gdl_tools texdef-inspect PATH_TO_TEXDEF NEW_METADATA_DIRECTORY
target/release/gdl_tools texdef-extract PATH_TO_TEXDEF PATH_TO_TEXTURES NEW_OUTPUT_DIRECTORY
```

With the established local cache and naming report, launch the native viewer:

```sh
# Requires the isolated virtual display described below.
python3 tools/python/run_viewer.py TITLE
python3 tools/python/run_viewer.py --scene LEVELS/levelG1
python3 tools/python/run_viewer.py --rig PLAYERS/SOR/YEL00
python3 tools/python/run_viewer.py --animate PLAYERS/SOR/YEL00
```

Left/Right select the source bitmap. A cycles source alpha, opaque RGB and an
explicitly labelled GS alpha hypothesis. Esc closes. Source pixels and metadata
remain unchanged; unsupported formats show a diagnostic. The TITLE package has
15 records; TITLE00 begins at record 11. The static scene uses original terrain,
base textures and vertex colors, with explicitly diagnostic opaque/double-sided
materials. The rig option shows a labelled translation-only rest hierarchy;
animated pose/skin semantics remain unverified. Arrows orbit, WASD pan, Q/E change
distance, P shows a detail view and R returns to overview. No native gameplay is implemented.

Independent research helpers require the separately pinned sources described in
[the upstream audit](docs/research/upstream-audit.md):

```sh
python3 tools/python/verify_wad.py
python3 tools/python/catalog_assets.py
```

The reference emulator uses its own private configuration and copied BIOS. It
is a development dependency only. Do not use the texture viewer to infer native
combat/campaign support. Gameplay, full animation acceptance, collision, saves, co-op and release
qualification follow the remaining master-plan gates.

For global PINE setup and a saved-state-preserving reference launch, see
[reference setup](docs/research/reference-setup.md). The launcher handles the
private configuration; its memory probe accepts the default slot 28011.
At the user's request all automated GUI work uses Xvfb; the launchers require
the recorded virtual session and never silently use the desktop:

```sh
xvfb-run -a -n 191 -e private/reports/xvfb.log -s '-screen 0 1280x1024x24 -nolisten tcp' python3 tools/python/virtual_display.py
# While that process runs, use another terminal:
python3 tools/python/reference_snapshot.py --state private/reference/captures/poison-fields.p2s --out private/reports/NEW_SNAPSHOT.json
python3 tools/python/reference_trace.py --steps 24 --keys Up --out private/reports/NEW_TRACE.json
# A bounded JSON list of {"steps":N,"keys":[...]} supports multi-segment scenarios:
python3 tools/python/reference_trace.py --schedule SCHEDULE.json --out private/reports/NEW_SCHEDULE_TRACE.json
```

The virtual display selects Mesa software presentation. See
[reference evidence](docs/research/reference-baseline.md) and
[geometry evidence](docs/research/geometry.md) for actual checks and limitations.
The static research reports can also be exported as glTF:

```sh
python3 tools/python/export_scene_gltf.py OBJECTS_REPORT WORLD_REPORT TEXTURE_EXPORT_DIRECTORY NEW_PRIVATE_DIRECTORY
```

Headless animation research commands preserve original channel payload identities:

```sh
target/release/gdl_tools anim-inspect ANIM.PS2 NEW_REPORT.json
target/release/gdl_tools anim-keys ANIM.PS2 TREE_INDEX SEQUENCE_INDEX NEW_KEYS.json
# Requests are [{"tree":0,"node":N,"sequence":S,"frame":F}, ...].
target/release/gdl_tools anim-samples ANIM.PS2 REQUESTS.json NEW_SAMPLES.json
python3 tools/python/reference_trace.py --animation --steps 24 --keys Up --out private/reports/NEW_ANIMATION_TRACE.json
python3 tools/python/compare_animation_keys.py --trace TRACE.json --key-dir KEY_REPORT_DIRECTORY --out private/reports/NEW_COMPARISON.json
```

The bounded native sampler matches measured Sorceress channels, including walking
and three attack clips. Source-qualified local matrices, clocks, bounded transitions and ordinary throw
request timing now have reference comparisons. Renderer scale/world integration
and complete events remain under recovery; the inspector is not playable. See [animation evidence](docs/research/animation.md).

The `--animate` inspector uses original clips and source-qualified local matrices.
PgUp/PgDn select clips, brackets step frames, and Space toggles a labelled preview
loop. The preview uses presentation time; original gameplay clocks/transitions,
world/VU composition and materials remain unaccepted. It is not a playable game.

```sh
target/release/gdl_tools anim-transforms ANIM.PS2 INITIAL_POSITION_ANIM.PS2 REQUESTS.json NEW_REPORT.json
python3 tools/python/compare_animation_poses.py --trace TRACE.json --animation-source ANIM.PS2 --rig-source INITIAL_POSITION_ANIM.PS2 --out private/reports/NEW_POSE_COMPARISON
```

Animation timing metadata uses `timing_factor`, not FPS: original InitAnim scales
this value into a period. Clock-state experiments can be reproduced with:

```sh
target/release/gdl_tools anim-clock REQUESTS.json NEW_REPORT.json
target/release/gdl_tools anim-clock-start START_REQUESTS.json NEW_REPORT.json
target/release/gdl_tools anim-select SELECTION_REQUESTS.json NEW_REPORT.json
target/release/gdl_tools player-anim-select ANIM.PS2 PLAYER_REQUESTS.json NEW_REPORT.json
python3 tools/python/compare_animation_clock.py --trace CLOCK_TRACE.json --metadata ANIMATION_METADATA.json --out private/reports/NEW_CLOCK_COMPARISON
python3 tools/python/compare_player_animation.py --trace ACTION_TRACE.json --animation-source ANIM.PS2 --out private/reports/NEW_ACTION_COMPARISON
```

The bounded player animation chooser matches measured walking, stopping and
throw transitions with original requested actions supplied. It predicts selected
actions/clips and clock states for supported source branches. Pad/action
production, gameplay events and complete game-loop timing remain open. See
[animation research](docs/research/animation.md) for bounds and evidence.

Add `--events` to the player comparison for the bounded ordinary Sorceress
throw-request timeline. This compares the event word and start timestamp; native
projectile creation, successful launches and hits remain unimplemented.

`gdl_tools world-matrix MATRIX_REQUESTS.json NEW_REPORT.json` evaluates the
bounded source-order world-matrix experiment. Stock debugger input/output capture
uses `tools/python/reference_world_matrix.py`; inspected entry/return screenshots
are required. See [world transform research](docs/research/world-transforms.md)
for the controlled-input distinction and current comparison limits.

`anim-samples`/`anim-transforms` requests can optionally include
`"transition":{"from":[nine prior channels],"fraction":F,"mirror":false}`.
The pose comparator supplies these observed values for nonzero reference blends.
Recorded walk-release blending now matches every local matrix/scale output; this
still does not predict actions or establish complete event timing.


Ordinary logical motion-intent/run-delay timer research can be reproduced with:

```sh
target/release/gdl_tools motion-code MOTION_REQUESTS.json NEW_REPORT.json
python3 tools/python/compare_motion_code.py --trace MOTION_TRACE.json --out private/reports/NEW_MOTION_COMPARISON
```

This bounded helper takes original logical controls and prior player context.
It predicts stored motion code and timer; native pad polling, requested-action
mapping, displacement and collision remain unimplemented. See
[clock/input evidence](docs/research/clock.md) for bounds and repeated comparisons.

PlayerMotion's ordinary pre-collision displacement request and facing turn,
using native reconstructions of the executable's own atan2f/sinf/cosf, can be
reproduced from a raw `--movement` reference trace:

```sh
python3 tools/python/reference_trace.py --schedule SCHEDULE.json --movement --out private/reports/NEW_TRACE.json
python3 tools/python/compare_motion_request.py --trace private/reports/NEW_TRACE.json --out private/reports/NEW_REQUEST_COMPARISON
```

The request is the value stored before collision stages; it is not native
movement. See [movement evidence](docs/research/movement.md).

## Interim playtest

An **interim** native walk-around (not the M12 slice) runs the Yellow Sorceress in
The Poison Fields with original assets and the native, reference-compared
movement and collision code:

```sh
python3 tools/python/export_start_state.py   # once; private start state from the save
python3 tools/python/run_play.py             # Arrows/WASD move, Space/K attack, Esc quits
```

There are no enemies, items, damage, HUD or audio yet. See
[playtest notes](docs/research/playtest.md) for exactly what is native and what is interim.

The [current checkpoint](docs/state.md) records exact progress, passing checks,
preserved reference state and ordered next steps.
