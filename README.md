# Gauntlet: Dark Legacy — native PS2 reconstruction

Implementation follows [the master plan](Gauntlet_Dark_Legacy_PS2_Rust_Bevy_Master_Plan.md).
The port is in development. Current native functionality is a bounded archive
import/extraction tool, original-texture viewer and static original-level inspector. Gameplay has not
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
cargo test -p gdl_formats -p gdl_tools --locked
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
```

Left/Right select the source bitmap. A cycles source alpha, opaque RGB and an
explicitly labelled GS alpha hypothesis. Esc closes. Source pixels and metadata
remain unchanged; unsupported formats show a diagnostic. The TITLE package has
15 records; TITLE00 begins at record 11. The static scene uses original terrain,
base textures and vertex colors, with explicitly diagnostic opaque/double-sided
materials. Arrows orbit, WASD pan, Q/E change distance, P shows the saved player's
vicinity and R returns to overview. No native gameplay is implemented.

Independent research helpers require the separately pinned sources described in
[the upstream audit](docs/research/upstream-audit.md):

```sh
python3 tools/python/verify_wad.py
python3 tools/python/catalog_assets.py
```

The reference emulator uses its own private configuration and copied BIOS. It
is a development dependency only. Do not use the texture viewer to infer native
combat/campaign support. Gameplay, animation, collision, saves, co-op and release
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
```

The virtual display selects Mesa software presentation. See
[reference evidence](docs/research/reference-baseline.md) and
[geometry evidence](docs/research/geometry.md) for actual checks and limitations.
