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

## Not yet

Attention-area music switching and stream parts (BGPlayStream), MapMusic/shop
music, the golem/boss enemy sound names (SetEnemyAudio 0x1d, modify_boss_ene),
AudioDemonFireBall, tower coin sounds, exact SPU volume/pan curves.
