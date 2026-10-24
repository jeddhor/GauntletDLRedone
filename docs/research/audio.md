# Audio (M17 work in progress)

## Resources (verified by parsing the retail files)

- `AUDIO/AUDATPS2.ROM`: header words +0x10 → bank table (0x2c bytes each: name,
  label, size, sound count +0x24, first name index +0x26), +0x14 → sound name
  table (0x1c bytes: name[0x10], stored ID +0x10, duration +0x14). 60 banks,
  2,228 sounds. IDs are `bank << 16 | call` (AudioTrack4 0x1224b8 uses
  `id >> 16` as the bank and `id & 0xfff` as the call).
- `AUDIO/*.VBK` ("VBNK" v1.06, read by IRX/DCS.IRX BankReadHeader): u32 call
  list bytes, u16 0x106, u16, u32 calls, u32 samples, then the call list as u16
  words and the VAGp samples. A call is a chain of sample words (low 12 bits =
  sample, 0x4000 loop start, 0x2000 loop back) ending with a 0x8000 word plus
  volume, duck and priority words (DCS.IRX BankPlay / voicePlayNextSample).
  Owned-data check: call order agrees with the ROM names, e.g. common call 0x42
  S_SUPERSHOT → sample "suprshot", sor 0 S_SORPAIN1 → "sorpn1", town 0
  S_ZOM1HITFAR → "zo1fhit".
- `STREAMS/*.ADS`: SShd (type 0x10 PS-ADPCM, rate, channels, interleave) +
  SSbd. TOWN1.ADS: stereo 24 kHz, interleave 0x20, 264.6 s; first 14 s decode
  without clipping.
- WDATA `SNDS` (0x18: name, id resolved by AudioFindSound, volume, priority)
  and `AUDS` (0x3c stage records: bank, enter sound +0x12, stream +0x18, areas
  +0x28, parts +0x2c) — The Poison Fields: town1, town2, town3a/b, town4_1/_2,
  town5. The save's G1 stage record (level_data+0x64) is "town1".

## Native implementation

- `gdl_formats::audio`: PS-ADPCM decoder, Vbk, AudioRom (AudioFindSound with
  15-character strncmp), Ads. `gdl_tools audio-inspect FILE REPORT`.
- `gdl_core::game::sound`: Audio* cue functions transcribed to their sound IDs
  (tables from the ELF: snd_throw, snd_pain, snd_dmg, snd_potion, world tables
  snd.260/347/362/366/483/539/543, pantab), runtime tables rebuilt as the game
  does (InitNameAudio names, SetEnemyAudio from the level's ENMY names, SNDS),
  AudioSFX3 volume × AudioVolMul. AudioPlayerPain and AudioPlayerEatSFX consume
  RandInt(4) as the original does.
- `gdl_client::audio`: ALSA mixer thread (presentation): sample chains at VAG rates,
  loops, queued voice lines, music loop; voice volumes, pans, ducking and
  voice allocation from the DCS voice model below. The SPU envelope, pitch
  skew, reverb and the hardware mix's saturation are not modelled.

- `gdl_core::game::bgmusic` (2026-10-21): BGMusicStart 0x15cc50 / BGMusicUpdate
  0x15cd40 / BGPlayStream 0x15c7a0 / BGMusicStop 0x15cd00 as a state machine
  in the simulation. The AUDS stage record gives the areas (stream letter
  suffix) and parts (`_n`); only an area's last part loops, and the host
  reports the end of a once-played part (PlayStreamComplete 0x120f88), which
  advances `streampart`. Volume: volMusic 100 × LEVL +0x94, eased 8 per frame;
  down to 3 before an area change with change mode 1; LowerBGMusic 0x15d0d0
  ducks it. Attention areas come from kind-0xd music items (+0xfc − 1 while a
  player is inside the +0xf0 radius, +0x100 the change mode) and boss
  activation. The map and shop play bank sounds S_MAP_<L> / S_SHOP_<L>
  (MapMusicStart 0x15cf00, ShopMusicStart 0x15cfd0; shop letters past K use
  A; no map music in the secret world or the Tower) and stop them at
  MapMusicEnd / ShopMusicEnd. `Event::Music{name,volume,looping}`,
  `MusicVolume`, `MusicStop` carry the stream to the host.
- Kind-0xd world sounds (ProcessItems → DoItemSound 0x15d2b0): the item name
  (+8, upper-cased) resolves through AudioFindSound's 15-character compare at
  load; each frame the nearest player's distance gives the level k (1 inside
  the radius or for radius ≤ 2, else 2·(1.5r − d)/r); while k > 0 the loop
  plays at 224·k × LEVL +0x98 (0x10 during Sumner's speech, trigger cameras
  and stop-time; world 12 level 0 ×4 clamped 0x40..0xff), priority 3 with
  +0x100 flag 2 else 0x70, +0x100 flag 1 ducks the music by 1 − k/2.
  `Event::ItemSound{slot,…}` keeps one tagged voice per item; `ItemSoundStop`
  ends it. Headless check (actual disc data): E1 slot 115 (radius 20, the
  player 0.2 away) emits ItemSound at 224 every frame
  (private/reports/game-run-E1-itemsound-v1.json); G1's six sources resolve
  (S_CROWS, S_PWHEEL, S_RIVERG, S_SFIREG, S_SWATERFALG, S_WNDMILL) but are
  100+ units from the start. Audible output is not verified (the test X
  server has no audio device).

- `gdl_core::game::audio_queue` (2026-10-21): the EE voice queue
  (AudioAnyQueAdd4 0x122ea0 with channel 0 = AudioQueAdd4, 1 =
  AudioVQueAdd4 while GoodWizard < 3; AudioQueUpdate 0x122ab0;
  AudioQueEmpty 0x122e58). Entries carry the ROM duration × 60 vblanks; a
  line is refused when it would wait longer than its max-wait; the head
  starts (→ `Event::Sound`) when its end time is 0 and retires at it.
  AudioStillPlaying = AudioQueUpdate's "entries left": it holds the level
  exit (game_main after do_players), the shop exit (do_shop), the map
  loader (do_mapscreen state 2), the good wizard (state 9) and the end of
  stage (world_update). main's AudioQueUpdate / AudioPoll run every frame
  in every mode (`Game::main_audio`; the client also ticks it while paused,
  in message boxes, the map and the shop). AudioSoundPlaying reads the
  started tracks' end times (Sumner's level speech). Per-call max-waits from
  the decompiles: AudioWithName (the first line: AudioExp / AudioWaitToExit
  5, AudioHasItem 4 / 1 for 0x1003d, AudioGainedLevel 3, AudioNeedFood
  1/1/0.5/2), player lines 1 (channel 0, panned by position), praise and
  rune hints 3, help 0.5, secret coin 1, speeches 10 (channel 1, not held
  by GoodWizard), runes found / stage entry none. AudioHasItem passes the
  item's sound as AudioWithName's extra line (`move a3,a1`); the `…1S` /
  `…2S` names are used while audio_select_state. The client's own queue
  was removed. Synthetic tests: serialisation and max-wait refusal, track
  ends.

- Golem audio (2026-10-22): SetEnemyAudio 0x15f1c8 type 0x1d names the
  sounds GOL<LevelLetter(0)> with kill word KILL and no bite, and looks up
  golwalk/golborn/golswing 0x23d440/44/48 = S_GOL<L>STOMP/BORN/SWING without
  modify_boss_ene; InitEnemyAudio 0x15f118 resets them to −1 per level.
  AudioEnemyWalk 0x15fa48 (types 0x1d/0x20: AudioSFX3(golwalk, 0x7f, 0x1c)),
  AudioGolemBorn 0x15fc68 (AudioSFX(golborn, 0xe0, 0xd)) and AudioGolemSwing
  0x15fc80 (AudioSFX3(golswing, 0xe0, 0xc)) are called from move_logic19.
  LevelLetter 0x162768 reads worlddatas[world_data_idx] +0xf (test world T →
  G for argument 0). Not exercised on the disc: the GOLEM enemy package is
  absent, so golems run as critters (gameplay.md).
- AudioDemonFireBall 0x15fcb8 (enemy type 2 fireballs by CurWorld) and
  AudioSelect 0x15ce80 are ported (effects.rs, sound_cues.rs).

## Audiovisual catalog (M17, 2026-10-23)

`gdl_tools av-catalog AUDATPS2.ROM SLUS_200.47 play-config.json
wad-independent.json NEW.json` (private/reports/av-catalog-v2.json, actual
disc): the 37 ELF cue tables the port reads (throw, pain, eat, door, tile,
barrel, rotator, generator, turbo, walk, menu clicks, map, tally, runes,
transport, legend items, Sumner's lines, player names …) hold 794 distinct
sound IDs, all resolving to an AUDATPS2.ROM record (bank, name, duration);
2,228 ROM sounds in 60 banks, each bank with its AUDIO/<name>.VBK on the
disc; per level, the AUDS stage bank (a ROM bank name or label — PYRAMID,
GENIE, SPIDER and CATHEDRAL are labels of DESERT, DJINN, DRIDER and CATH),
its 106 music streams (BGPlayStream's names by area and part) all present in
STREAMS/, its 279 named SNDS world sounds all found by AudioFindSound, and
every LEVL movie (+0x34) present under MPEG/. This catalogues event → asset
bindings; event selection and timing are checked by the whole-update
replays (gameplay.md: sound events are Game::events from the ported Audio*
functions).

## DCS voices and the volume/pan chain (2026-10-23)

Ported from instructions (EE: private/reports/ghidra-audio2/3/5/8; IOP
DCS.IRX: ghidra-dcs1/dcs2):

- EE. AudioSFX 0x1227f0 / AudioLOOP 0x1229e0 take the pan from AudioAng
  0x1218a0: the x/z direction from Cam[0]'s attention point (+0x140) to the
  source, normalised (NormalVector), dotted with Cam[0]'s first matrix row
  (+0x10) × 127.5 + 127.5, truncated, negated when row.x · dir.z <
  row.z · dir.x, clamped to −0x100..0xff; 0x7f without a position or with
  mono_flag. AudioSFX2 and the queue carry fixed pans. AudioTrack4 sends
  (volume × volSFX >> 8) << 16 | pan (0x7f when mono) and the priority.
  DoItemSound updates its loop each frame with AudioTrackVol (volume ×
  volSFX >> 8) and AudioTrackPan(AudioAng). volSFX 0x21f6c4 (data 128),
  volMusic 0x21f6c8 (data 100) and mono_flag 0x21f6f4 (data 0) are set by
  OptionsSetup 0x10a5c0 from GamePrefs words 0, 1 and 2 (AudioSetVolSfx /
  AudioSetVolMusic clamp to 0..255; AudioSetStereo: mono = word 2 == 0).
- IOP (gdl_core::dcs). BankPlay 0x5b10: voiceFind 0x4864 over 12 voices
  from a round-robin start (a free voice — no next sample, no duck held,
  envelope off — else a voice whose priority word, request priority << 16 |
  call priority, is a new running maximum not above the raw request
  priority, i.e. in practice the request is dropped); volume = (request
  volume × call volume) / 0x7f − gVolDuck, pan word kept; a call with a duck
  word runs voiceDuck 0x43f4 (gVolDuck += duck × gVolDuckScale >> 8 —
  0x100 from aud_init's RPC 0x13 block — and every voice in use loses it),
  undone at the chain's end, a stomp or VoiceStop. VoiceVolume / VoicePan
  move a voice by at most 8 per call (pans the short way within
  −0x200..0x1ff). voiceSetVolPan 0x39d4: L = R = 0x3fff × max(vol, 0) / 255
  (VagParseHeader's base 0x3fff; overflow folding above 0x3fff), then L ×
  f_L >> 14 and R × f_R >> 14 into 15-bit VOLL/VOLR, the factors from the
  quarter-sine table at DCS.IRX 0xb188 (u16[256], SHA-256 ac069de9…, read
  from the user's IRX by identity) by the pan word's mode (bits 8..9): 0 →
  (sin[255−p], sin[p]); 1 → (−sin[p], sin[255−p]); 2 → (−sin[255−p],
  −sin[p]); 3 → (sin[p], −sin[255−p]) — AudioAng's negated pans (sources
  behind the camera) phase-invert the right channel.
- Music: aud_stream_volume 0x123fb0 sends 0x55ab, v | 0xffff0000; DCS sets
  AdsSetVolume((v & 0xffff) × 0x1fff >> 8) per channel (stereo: each ADS
  channel on its side; mono output mode: both sides × 181/256).
- Evidence: synthetic tests (gdl_core dcs, gdl_client audio) and the
  actual-disc table (dcs-pan-v1: identity and the words for representative
  pans); no SPU capture compares the mixed output.
- Movie soundtrack level: audioDecResume 0x1de4e8 starts the stream with aud_stream_play(volMusic, 0, 1, buffer), so the soundtrack takes AudioStreamPlay's volume word of volMusic (native: AudioOut::play_pcm, 2026-10-23).

## SPU voice setup (2026-10-23)

DCS.IRX (ghidra-dcs2/dcs4, instructions read): samplePlayOnVoice 0x3c7c
sets every voice to ADSR1 0x000f (linear attack rate 0, decay rate 0,
sustain level 0xf = maximum) and ADSR2 0x1fc0 (linear sustain rate 0x7f —
no change — and linear release rate 0): an on/off rectangle, which the
native mixer's plain sample playback equals. Pitch: VagParseHeader 0x3410
converts the VAG rate to rate × 4096 / 48000 (the SPU's 0x1000 = 48 kHz)
and samplePlayOnVoice shifts it by pitchSkew. Reverb: InitSpuParams 0x88a4
sets only the cores' master volumes (params 0x980/0xa80 = 0x3fff); no DCS
code sets effect volumes or voice effect switches, so nothing is mixed into
reverb. pitchSkew (0xb13c, 0 in the data) is toggled only by skew_correct
0x605c — a hidden 16-step button-sequence check on the levels the EE sends
with RPC 0x55af — but aud_poll 0x1238a8 sends them only while SIF register
0x1e is positive (blez on the sign-extended word) and every write of that
register is ≤ 0 (EE aud_init 0, aud_reset −2; DCS's file open −1): the
toggle is dormant in this build and not ported. Remaining SPU differences
not modelled: the SPU's 4-point Gaussian resampling and its 16-bit mix
saturation.

## Audio wrappers audit (2026-10-23)

Every EE Audio* function was checked for a native counterpart (by name, or
its sound request inlined at the native caller; scratch audit of the
wrappers' decompiles, private/reports/ghidra-audio10/11/12). Found missing
and ported from instructions:
- DoWaveEndAudio 0x177848 (do_exit, after AudioPlayerExit): outside worlds
  5 and 8 the level's quest item or rune nobody holds queues one of
  Sumner's four lines (snd.566 0x23dda0 = 0x10057, 0x10045, 0x10058,
  0x10047; snd.571 0x23ddb8 = 0x10055, 0x10046, 0x10056, 0x10048; indices
  cycling, never reset): AudioVQueAdd4(−1, 0.5, line, 0xe0, 0x7f, 2).
- PlayerProcessPowerups' AudioUnpojo 0x160580 (99) and AudioLevitatedown
  0x1605e8 (0x59) when the pojo (+0x138 0x400) or levitation (1) wears off;
  game_main's AudioUnshrink 0x1605d0 (0x55) when EnemyShrink grew back.
- ProcessItemWobjs 0x154790's sounds: bridges (AudioBridgeOpen 0x160860 /
  Close 0x1608c8, snd.475/479 per world), trap switches (AudioTrapSwitch
  0x1607e0, snd.471 two rows of 14 worlds) and elevators (AudioElevator
  0x160148: AudioWorldInit 0x15c558's loop/stop names "S_ELV<MET|ROPE|
  CHAIN|ICE|STONE><L>[B]" / "…STP…" and "S_ROCKROTATE"/"S_ROCKSTOP"; the
  loop at priority 0x73, killed when no elevator runs; its pan update is
  presentation).
- AudioMenuBurn 0x1606e8 (0x13) when a scroll box starts burning.
Already present inline: AudioDeathShatter/Laugh, AudioEnemyWeaponHit,
AudioWeaponHitBarrel, AudioEnemyShootArrow, AudioSuicideYell/Bomb,
AudioTimeBomb, AudioTentacle, AudioSplash, AudioOpenChest, AudioPlayerExit.
No-ops or not reachable: AudioGameVolume / AudioAttractVolume (empty),
AudioEnabled (NO_AUDIO). Not yet: AudioPlayerXray (do_see_thru, the x-ray
chest view, presentation not ported), the option menus' burn on close
(end_optmenu → StartFireScroll; the native menus close without the fire
scroll), bank management (AudioLoadBank/ReplaceBank/UnloadPart: the host
loads banks on demand). Replays r51 = r50 (sounds are events).

## Reference output capture (2026-10-24)

PCSX2's System → Video Capture records the SPU2 output (after its mix
clamp and output DC filter) in emulated time, AAC 48 kHz stereo
(reference_driver.py ["capture", "start"|"stop"]; private capture
ref-audio-g1-v1.mp4: the Poison Fields fixture, player idle, 86.5 s).
tools/python/audio_compare.py locates a window of it in the native decode
of an ADS stream (gdl_tools ads-pcm) by cross-correlation and fits gains:
the fixture's music is TOWN1 (correlation 0.966 left, 0.967 right over
8 s; TOWN2 0.10), and the spectrum matches the native decode within
±0.6 dB from 90 Hz to 3.5 kHz; above, the capture falls (−2.2 dB at 7 kHz,
−5.7 dB at 11 kHz, −10.5 dB at 14 kHz: the SPU's 4-point interpolation and
the AAC encoder), below 60 Hz the output DC filter takes up to 2.7 dB
(private/reports/audio-compare-g1-TOWN1-v1.json).

Level (open): volMusic is 128 in the captures (0x21f6c8) and G1's LEVL
+0x94 factor is 1.0, so the stream volume word is 128 — AdsSetVolume gain
(128 × 0x1fff >> 8) / 0x4000 = 0.25 of full scale, which the native mixer
outputs — but the capture fits 0.0754 (the word 39's gain): the reference
chain after AdsSetVolume carries a further ×0.30 (−10.4 dB) on the music
that native lacks.

Music/effects balance (2026-10-24): a live 15.2 s capture of B4 from
lvl-B4-start-v1 (no input: trace-B4-start-v1's schedule; private
ref-audio-b4-start-v1.mp4) against the native frame replay of that trace
with its per-update events (GDL_REPLAY_EVENTS) rendered offline through the
native mixer (crates/gdl_client/src/bin/gdl_audio_render.rs: play_audio's
dispatch, one update = 2 × 48000 / 59.94 frames; music and effects rendered
apart; volSFX 128 as the reference's 0x21f6c4). The music (MOUNT4, located
at 3.62 s by cross-correlation) fits gains 0.062 / 0.064 against the
native 0.25 at word 128 (×0.25); with it removed, the remaining effects
(chiefly the looping item sound on slot 206, panned left) measure 0.0558 /
0.0207 RMS against the native render's 0.190 / 0.073 (×0.29 / ×0.28). Music
and effects are scaled alike, so the balance agrees within about 1 dB; the
whole reference output sits about 11 dB below native (G1's music ×0.30).
PCSX2 applies volume registers as (value << 1) × sample >> 15 (ADSR.cpp
V_VolumeSlide::RegSet, Mixer.cpp ApplyVolume; upstream reading), so 0x3fff
is unity there as in native's VOLL / 0x4000; the common factor's stage is
not identified (open; a hardware recording would settle it). Waveform-level
fits of the effects are not meaningful here: the looping voices of the
saved state keep their own phase.

## Not yet

Tower coin sounds, the output level and music/effects balance against the
reference capture (above), the SPU's interpolation filter (provenance:
unknowns.md), AudioLoadStage bank residency (the host loads banks on
demand). Sound items follow their +0xf8 world node (process_sound_item).
