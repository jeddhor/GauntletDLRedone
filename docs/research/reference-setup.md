# Reference IPC setup investigation

Installed reference: PCSX2 2.6.3+dfsg-1build1. The checkout supplied by the user
is a different revision, so this UI investigation also inspected official tag
v2.6.3 at commit bc8151d2a46d4aba039ea5580afbfc7bfcf6d730.

The user's disabled Slot field has a concrete source explanation: in the
[v2.6.3 setting binder](https://github.com/PCSX2/pcsx2/blob/v2.6.3/pcsx2-qt/SettingWidgetBinder.h),
the nullable integer QLineEdit accessor disables the widget when binding a
per-game override. The PINE slot is such a QLineEdit. The observed private
game-specific INI contained EnablePINE=true, while the global INI still had
EnablePINE=false and slot 28011. PINE must be configured in the global settings
for this reference workflow; the disabled slot does not prove a general ban
on editing settings while a game is loaded.

The [v2.6.3 VM manager](https://github.com/PCSX2/pcsx2/blob/v2.6.3/pcsx2/VMManager.cpp)
calls ReloadPINE during startup and disc changes. Its ordinary settings-apply
path has no ReloadPINE call. Use a saved-state-preserving restart after enabling
global PINE, rather than assuming a live checkbox change starts the server.
Hardcore mode also forces PINE off, but the established private configuration
has achievements and challenge mode disabled; it does not explain this case.

tools/python/launch_reference.py sets global EnablePINE and the selected slot
inside private/reference/config/PCSX2/, preserving backups and unrelated settings.
It removes only PINE overrides from the private game-specific INI. It refuses
to modify/relaunch the recognized reference while that process is running.
A supplied existing private state starts paused. The launcher verifies the
disc hash and records the configuration/state identities before launching.
Synthetic tests cover the global settings, backups and preservation of unrelated
preferences. The state-preserving live relaunch and frozen/live probes now pass.

The default slot 28011 is sufficient. On Linux its socket is pcsx2.sock;
other slots append .<slot>. The memory probe now reads the configured global
slot, so changing to 28012 is optional.

The user saved private/reference/captures/poison-fields.p2s. It is verified and
preserved as poison-fields-original.p2s. All automated GUI work now runs through
authenticated Xvfb at the user's request. Launch helpers require this session;
they cannot fall back to the desktop. Start the virtual display and then reload:

```sh
xvfb-run -a -n 191 -e private/reports/xvfb.log -s '-screen 0 1280x1024x24 -nolisten tcp' python3 tools/python/virtual_display.py
# In a second terminal, while that virtual session is running:
python3 tools/python/launch_reference.py --diagnostic-hotkeys --state private/reference/captures/poison-fields.p2s
python3 tools/python/pine_probe.py --out private/reports/poison-fields-pine.json
```

Xvfb requires Mesa software presentation (lavapipe/llvmpipe), selected only in
the child process environment. The physical AMD Vulkan presentation queue cannot
present to this virtual server. Reference configuration identities are recorded.
F10 advances a frame; F7 loads private slot 1; F11 saves private slot 2.
The bounded trace tool verifies one vblank per advance and releases held input.
See reference-baseline.md for actual results and remaining timing limitations.

The user is not expected to change their normal emulator configuration or saves.
All of this is isolated development-reference tooling; native gameplay will not
depend on PCSX2, PINE or a BIOS.

Virtual-display guard follow-up: the authenticated xvfb-run wrapper can exit while
its Xvfb server and Python holder remain alive under user systemd. The holder's
current parent is insufficient as a lifetime check. The session now records and
verifies actual Xvfb PID/start-time and exact display/auth arguments on every
GUI operation; it still refuses Xorg/Xwayland or an authentication/PID mismatch.
The existing :191 server was independently verified during this migration.
A synthetic process fixture covers these rejection cases. No desktop fallback.

## Warped level-start captures (2026-10-22)

tools/python/reference_warp.py reloads a preserved slot, can write player 0's
position (the record's matrix translation +0x50 and its node's local
translation +0x30 — a guest write, so these states are labelled as warped),
steps a bounded key schedule with verified F10 vblanks, and saves the result to
a new PCSX2 slot (PINE MsgSaveState 0x09; slots 1 and 2 and existing files are
refused) copied to a new private capture. From slot 2 (the Tower new game),
standing on a realm portal exits after ~300 vblanks (CurWorld changes, player
state 5), the realm's first level loads through game modes 0x400f/0x400e and
starts with no map screen. Captures (private/reference/captures, reports
warp-*.json and screenshots in private/reports):

| Slot | Capture | Portal (Tower exit) | Level at save |
|---|---|---|---|
| 3 | w2l1-newgame-v1.p2s | (−25.55, −6.43, −117.13), world 2 | B1 after its start (1200 vblanks) |
| 4 | w1l1-newgame-v1.p2s | (117.93, 0.59, −88.83), world 1 | A1 "Courtyard" during the stage-start title and zoom |
| 5 | w11l1-newgame-v1.p2s | (163.11, 5.91, 42.41), world 11 | still loading (mode 0x400e) — not for traces |
| 6 | w7l1-newgame-v1.p2s | (45.07, −6.43, −112.70), world 7 | G1 new game after its start |
| 7 | w11l1-newgame-v2.p2s | world 11, 1800 vblanks | K2, the moving ship (SKY's LEVL record 0), after its start |
| 8 | w2l6-boss-v1.p2s | from slot 3: player 0 written onto B1's exit (−8.578, 139.0, −68.06) and skip_wave = 0x10205 (do_exit's wave skip to MOUNT record 5), 1400 vblanks | B6, the dragon's arena, in play (exit → map 0x400f → load 0x400c → 0x4010 by ~1400) |
| 9 (staging) | w7l5-boss-v1.p2s | from slot 6: player 0 onto G1's exit (118.28, 86.8, −472.5), skip_wave 0x10704, 1000 vblanks | G5, the lich's crypt, in play |
| 9 (staging) | w<world>l<level>-boss-v1.p2s (C5 w3l5, D5 w4l5, E2 w5l2, F2 w6l2, H4 w8l4, I5 w9l5, J5 w10l5, K5 w11l5, A5 w1l5) | from slot 3: player 0 onto B1's exit, skip_wave 0x10000 + the boss wave, `--until-wave` (1000..1400 vblanks) | the boss level in play, boss not yet active (reports warp-<level>-boss-v1.json) |
| 9 (staging) | w9l5-boss-v2.p2s (sha256 793dedef…) | from w9l5-boss-v1: player 0 written to (9.6, −4.047, −5.2), 62 units from the yeti (a floor point by collision-query), 4 vblanks | I5, WARPED position (guest write); for the yeti's activation |

Slot 9 is a staging slot (tools/python/reference_slots.py): `--stage NAME`
copies a preserved capture into it for reference_trace.py / reference_warp.py,
and it is overwritten (by a stage or a `--save-slot 9`) only while it holds a
byte-identical copy of some preserved capture, so no state is ever lost.

reference_trace.py and reference_screenshot.py accept these slots; each run
first checks the PCSX2 slot file against its preserved capture.
