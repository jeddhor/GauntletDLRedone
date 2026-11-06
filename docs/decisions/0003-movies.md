# 0003: Movies — host-decoded video, natively decoded audio

Superseded for the video by 0007-native-movie-video.md (2026-11-05): the
client decodes the video itself; FFmpeg is no longer used at run time.

The disc's 36 movies (MPEG/*.PSS, MPEG2/*.PSS) are MPEG-2 program streams:
640×480 MPEG-2 Main-profile video at 30 fps (PES 0xe0) and a private stream
1 (0xbd) whose payloads, after a 4-byte substream header, form an SShd/SSbd
PS-ADPCM stream (24000 Hz stereo on most files; FSCROLL 44100 Hz, MOVIEC1
22050 Hz; interleave 0x20). The original decodes the video with the IPU
(InitMovie/ServeMovie/KillMovie, libmpeg) and the audio with its own
decoder.

Decision: the native client decodes the video with the system ffmpeg as a
child process (raw RGBA frames over a pipe; `GDL_FFMPEG` overrides the
executable) and decodes the audio natively (gdl_formats::pss over
audio::Ads). The game logic (init_gamemovie/do_gamemovie, the attract
sequence) stays in gdl_core and never depends on the decoder. Without
ffmpeg, or when a file is missing, a movie ends at once — the path the
original takes when InitMovie cannot open the stream — and play goes on.

Why: an MPEG-2 decoder is a large, well-specified, non-game component;
reimplementing the IPU decode path is not part of the reconstruction
contract, and decoding at extraction time would store gigabytes of frames.

Verified toolchain: ffmpeg 8.0.1-3ubuntu2 (/usr/bin/ffmpeg). Colour
conversion is ffmpeg's BT.601 limited-range YUV→RGB (HYPOTHESIS: the IPU CSC
gives the same values; not compared with a reference capture). Frame
pacing: frame k shows at k/30 s from the start of playback.

Release scope: ffmpeg becomes an optional runtime dependency of the desktop
client (movies only). Reversible: a native MPEG-2 decoder can replace the
child process behind gdl_client::movie without touching the game logic.
