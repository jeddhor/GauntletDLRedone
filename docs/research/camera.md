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
  and 60/60 StdCam records bit-exact. The transmitter did not change in
  this trace, so the rotation path is not yet trace-verified.
