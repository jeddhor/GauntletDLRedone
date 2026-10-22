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
  loops, queued voice lines, music loop. Pan, ducking, voice stealing and the
  SPU envelope/pitch are approximations.

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

## Not yet

Tower coin sounds, exact SPU volume/pan curves, the
+0xf8 world-node motion of moving sound items (the position stays the
placement), AudioLoadStage bank residency (the host loads banks on demand).
