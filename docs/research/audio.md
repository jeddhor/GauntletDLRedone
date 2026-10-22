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

## Not yet

The golem enemy sound names (SetEnemyAudio type 0x1d, guarded),
AudioDemonFireBall, tower coin sounds, exact SPU volume/pan curves, the
+0xf8 world-node motion of moving sound items (the position stays the
placement), AudioLoadStage bank residency (the host loads banks on demand).
