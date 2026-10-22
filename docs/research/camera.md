# Gameplay camera (StdCam)

Revision: SLUS-20047 (profiles/ps2/slus-20047.json), EE address space.

## Which camera runs

`main` dispatches the camera after the game logic (0x100600..0x10075c):
game modes 0x8009/0x400b/0x4012 call MBCameraUpdate on a fixed camera;
0x4013/0x400d/0x4017 and boss levels (BossCritInst ≠ 0) use BossCameraUpdate
0x1a1bc0 (falling back to do_camera); game_mode & 0x8000 uses do_camera
0x1738e8; otherwise, while `new_camera_switch` 0x20eaa0 ≠ 0, StdCamUpdate
0x115fe8 (newcam_mode 0x21f130 = 2), else do_camera. `new_camera_switch` is
1 in the ELF and no instruction writes it (memory-reference scan), so the
StdCam module (0x113358..0x116720) is the gameplay camera. The older Cam
array (0x23e530, StandardCamera 0x171e28) still runs for trigger cameras
(TriggerCamUpdate 0x1a26a8 → do_camera) and bosses; it is not ported.
Both captures hold newcam_mode 2.

## StdCam record (StdCamera 0x2bc740, 0x1b0 bytes)

+0x00..+0x3c camera matrix (right, up, forward rows; row 3 = eye),
+0x40..+0x7c four frustum side normals, +0xa4 target, +0xbc stage-start
target step, +0xdc max radius, +0xe0 look direction, +0xec yaw, +0xf0 yaw
step, +0xf4 distance, +0xf8 distance step, +0x104 pitch, +0x108 pitch step,
+0x10c horizontal view angle (CamReset: 0x3f860a92 = 60°), +0x110 aspect,
+0x114 nine target history entries, +0x180 nine distance history entries,
+0x1a4 history index, +0x1a8 transmitter of the current rotation, +0x1ac
rotation progress. Constants 0x21f0d8..0x21f140 are read from the ELF
(CAM_HIST_SIZE 9, MAX_ZOOM_FRAMES 10, ZOOM_ADJUST 10, MAX_ZOOM_ADJUST 4,
ADJUST_SPF 1, ADJUST_MAX_RAD 0.9375, ADJUST_MAX_RAD_1PLAYER 0.8,
CAM_ROT_FRAMES 50, CAM_EPSILON 0.0001, camPerformanceTest 0); both captures
hold the same values.

## Update (UpdateCam 0x116070)

1. CamGetPlayerAvgPos(5): screen-space (MBWorldToScreen) min/max of the
   valid players' +0x60 positions (CamIsPlayerValid: +0x93c bit 0x20 clear,
   state +0xfc 1 or 4), midpoint back through MBScreenToWorld, clamped to the
   CAMS box; pushed into the target history.
2. CamGetPlayerAvgPos(2) (+0x50, world space) selects a transmitter with
   GetTransmitter3D 0x1143a8: the nearest flag-0 transmitter other than the
   current one replaces it when its squared distance ≤ 0.44444445 × the
   current one's. With no other candidate the current index becomes −1 and
   the table's preceding words are read (native: Unsupported).
3. TransGetYawPitch: yaw = wrap(transmitter yaw − π), pitch = −pitch (more
   than one player: pitch ≤ −CAMS +8).
4. LimitYawPitch: on a transmitter change, steps = wrapped difference / 50;
   applied × SPF·30 per update until the progress reaches 50.
5. LimitTranslate / LimitDist: target and distance move by the mean
   difference to their nine history entries.
6. Max radius: one player → CAMS +0x2c × 0.8; otherwise the transmitter's
   radius byte or CAMS +0x30 × 0.9375. CalcDist: one player → CAMS +0x2c;
   more players fit all players inside the frustum normals.
7. CamUpdate: look = PitchVec3(−pitch, YawVec3(−yaw, (0,0,1))), DoShake,
   eye = target − distance × look, CamLookInDir, MBCameraUpdate (window
   camera rows; MboxWinPtr +0x90/+0x98 = forward x/z for PlayerMotion),
   MBWindowSetHang / MBWindowProjection.

The frame's MBEndFrame → pbWinSetup recomputes the pb matrices (pbProjCalc,
pbCameraCalc: sceVu0CameraMatrix + sceVu0InversMatrix, calcMatrices) from
the new eye. MBWorldToScreen uses the combined +0x2d0 matrix as last
computed, recomputing only the dirty projection/camera parts.

## Level start

init_next_level → init_game_cam 0x16fec8: a CAMS record whose +0x24 byte is
0 gets the world box (WorldInfo +0x18/+0x24 = WORLDS header bounds) inset
by (8, 0, 8) / (−8, +4, −8); StartTransmitter +2 = 0; with a start
transmitter (locator kind 1) and game mode other than 0x8006/0x8003,
cam_wait = 0x5b and start_of_stage = 1. StdCamInit(0) resets the record and
CamInit places it at the start transmitter looking along its direction, the
target at the player's distance. In mode 0x4010 StdCamStartStage clamps
cam_wait to 2 (no camera update while it is positive); world_update sets
cam_wait = 1 three seconds after WorldStartTime; then StdCamStartUpdate
zooms over MAX_ZOOM_FRAMES updates toward a stack record settled by
CalcDestination and finally clears start_of_stage and cam_wait.
HYPOTHESIS (native): that stack record's untouched bytes start at zero.

## Transmitters (AddLocatorInstList 0x150ac8 / AddTransmitter 0x150768)

Locator kinds 1/2 → flag 0 (kind 1, the start transmitter, is re-marked 3),
3/4 → flag 1 (attention; pitch negated instead of yaw + π), 9 → flag 2
(trigger camera, LinkTriggerToCam). +1 radius byte = locator +1, +2 = 1,
+0x10 position, +0x20 pitch, +0x24 yaw (FixAngle(yaw + π) unless flag 1),
+0x28 roll, +0x30 arrow tree (debug arrows, presentation).

## Player limit (NewCamLimitPlayerDpos 0x116358)

PlayerMotion → CameraLimitPlayerDpos 0x1a2428 → StdCamLimitPlayerDpos (while
newcam_mode is 2): the displacement is clipped against the record's frustum
normals through target − max radius × look (+0x70 against planes 0..2, +0x50
against plane 3); returns 5 when a plane was crossed (stored at +0x978).
Ported as `Camera::limit_player_dpos`; it is wired in with the full
PlayerMotion port.

## Window parameters

The pb window values (frame 640×448, screen 512×384, depth 1..0xffffff,
offset 2048/2048, near 1, far 65536, viewport insets 0/0/0/64, up (0,1,0),
MboxWinPtr height 384) are the values both captures hold; MBWindowInit
0x1d0fc8 sets the frame/height defaults, the source of the 64-pixel bottom
inset and the 384 height is not yet identified. Other player counts are a
HYPOTHESIS.

## Verification

- private/reports/camera-check-g1-v2.json, camera-check-tower-v2.json
  (actual disc, frozen captures): all 140 / 142 transmitters and both
  derived CAMS boxes bit-exact.
- private/reports/trace-stdcam-g1-v1.json: 120 verified vblank steps of the
  slot-1 fixture with scripted movement (reference_trace.py --stdcam).
  `gdl_tools stdcam-replay` starting each update from the previous traced
  record: 60/60 pb blocks (view, inverse, projection, viewport, combined)
  and 60/60 StdCam records bit-exact (no transmitter change).
- private/reports/trace-stdcam-g1-v2.json: 500 vblanks (walk across three
  transmitter zones, 7 → 114 → 24 → …): 250/250 pb blocks and 250/250
  StdCam records bit-exact, including the 50-step rotation blends.
- Not trace-checked: StdCamInit(0) / the stage-start zoom (the fixture is
  mid-level), more than one player, DoShake.

## Native integration

`Game::std_camera` is built at level load (transmitters from the WORLDS
locators, CAMS from WDATA, constants from the ELF). `Game::start_camera`
runs init_game_cam's stage-start part and StdCamInit(0) once the players are
placed; `Game::start_camera_settled` (HYPOTHESIS) uses StdCamInit(1) for runs
that start from a mid-level fixture. Each update ends with StdCamUpdate and
the pb recompute; MboxWinPtr +0x90/+0x98 (PlayerMotion's camera yaw) and the
MBWorldSphereClip view (eye, rows, target; planes from the start state's
window constants) are taken from it. world_update's start-of-stage block
(StartEnterFX header 0x43, AudioPlayerEnter = AudioFX(2, 0xe0, 9), cam_wait 1
after 3 s), do_enemies' stage-start idle actions and do_players' stage-start
branch (no PlayerMotion while cam_wait and start_of_stage are set) are
ported. The client draws with the pb projection (separate horizontal and
vertical tangents) in the top 384/448 of the window, matching the reference
layout where the player panels occupy the bottom 64 rows.

## Boss camera (BossCameraUpdate 0x1a1bc0)

Used while BossCritInst ≠ 0 (and in the boss-select modes 0x4013/0x400d/
0x4017); newcam_mode is then 0. BossCameraInit (reset_game_data, every level)
resets the BossCamera record with CamReset and BossCameraMode 0. The level's
BCAM record (WDATA BCAM, 0x54 bytes, LEVL +0x8c) holds flags (+0), an angle
(+4) whose cosine BossCameraStart stores at +8, distances (+0xc minimum for
the boss calculator, +0x10 minimum for the player calculator, +0x14 boss
distance, +0x18 player distance), pitch range (+0x1c/+0x20), near/far
attention offsets (+0x24/+0x30, lerped by +0xfc), key and good-wizard
offsets (+0x3c/+0x48). Every update: first BossCamStartCalc (camera from
the start transmitter's direction; sets start_of_stage), then
BossCamPlayerCalc while BossActive is 0 (GetTransmitter's 2-D choice with
the 0.667 switch, view-distance zoom from PointInView margins) or
BossCamBossCalc (attention between boss/key/wizard, the two farthest
entities' direction via GetActualAvgVec, BCAM-flag-controlled facing
limits), BossCamLimitAttn / BossCamLimit (LimitCamVal2 velocity limits with
CAMMAX*/CAM*ACCEL from the ELF), then yaw/pitch wrap, look vector, DoShake,
eye, LookInDirection and MBCameraUpdate (no MBWindowSetHang). While
start_of_stage is clear BossCameraUpdate clears cam_wait. Instruction-level
review notes: the decompiler misorders the zoom-in step (the code adds
2·(2.5 − view distance) to the distance); `GetPlayerAvgPos` returns
1/count, which BossCamBossCalc passes to GetBossAvgPos as its weight;
`|d|/|v|` in LimitCamVal2 divides by zero when the velocity is 0 (EE result
±FLT_MAX, `div_rn_ee`).

Transmitters[−1]: GetTransmitter3D / GetTransmitter return index −1 when
the only candidate is the current one (B6 has a single camera transmitter,
so the original alternates every update). The table's preceding words are
PlayerInitStartPos entries 9–13: +0x24 is entry 12's y (−100000, set by
AddLocatorInstList each level), +0x20 entry 12's x (0: no level, the Tower
included, has a start locator with id 12), +1 byte 1 of entry 9's y.
`transmitter_before_table` reproduces those reads.

Cur3DTransmitter 0x21f120 and CurTransmitter 0x21f11c are never reset
(only GetTransmitter3D/GetTransmitter write them), so the previous level's
index carries over (`Globals::cur3d_transmitter`, `cur_transmitter`).

good_wiz_objgrp +0x40 (the wizard's attention point, set by DoGoodWizard's
state 3) feeds the kind-2 view once the node exists. After BossDeath,
BossCritInst still points at the freed instance and BossCamBossCalc keeps
reading it; natively the instance's last record and type radius stand in
(HYPOTHESIS: the pool does not reuse it before the level ends). The boss
key effect is taken as living until the level ends (BossKeySfx's effect
lifetime is not tracked, HYPOTHESIS).

Presentation fix (2026-10-21): the window camera is drawn from
MBCameraUpdate's pb eye and rows; the client had taken the eye from the
StdCam record, which BossCameraUpdate does not write, so boss arenas were
drawn from the level-start eye with the boss camera's rotation (the player
at the screen's top edge in A5).

Not trace-checked: no capture or trace of a boss arena exists yet.

## Trigger cameras

TriggerCameraActivate 0x1a2550(wobj, position, angles, delay, wait, hold):
delay < 0 → 1000000, 0 → 0x28, else × 6 vblanks; trig_cam_active = 2;
DisablePlayerControls (PlayerControls 0x105710 zeroes every player's
controls while disable_player_controls is set); trig_cam_mat = LookInDirection
of PitchVec3(pitch, YawVec3(π − yaw, (0,0,1))) with the position as eye.
TriggerCamUpdate 0x1a26a8 (before main's camera dispatch): wait counts down
first (normal camera meanwhile), then each update StdCamFreeze and
MBCameraUpdate(trig_cam_mat) with letterbox bars (presentation); after the
delay, the hold and the bound object's animation (+0x10 bit 0x8000000) the
camera ends, EnablePlayerControls, trig_cam_active 0 and do_camera() (old
Cam array, not ported); the next StdCamUpdate re-applies the gameplay camera
(StdCamReturn).

LinkTriggerToCam 0x150328 (kind-9 locators): in the Tower ids 0xc6 crystal,
0xc9/0xcb/0xcd rune, 0xca/0xcc window, 0xf0.., 0xdc.., 0xaa.. three Sumner
tables of 14; elsewhere trigger items (definition kind 5, signed byte +0xf6
= id) get +0x102 = the transmitter index, and ProcessItems activates
TriggerCameraActivate(bound object, transmitter, its radius byte as delay,
wait 0x1e, hold 0) when the trigger fires. CrystalCamActivate uses delay
0x32; SumnerCamActivate holds until TriggerCameraEnd; Rune/WindowCamActivate
hold 300 (0x2a7f98 / 0x2a7fa8). Actual-disc check
(camera-check-tower-v3): all 48 Tower links and the crystal camera's
trig_cam_mat bit-exact against the capture.

ShakeCamera call sites (radius, type, delay, count, priority): PlayerMotion
(0.3, 0, 0, 0x1e, 0xc8), DoPlyrSfx and CritterDoSfx (0.1, 0, 0, 0x5a, 0x64),
ProcessItems triggers (0.1, 0, 0, 0xb4, 0x64).
