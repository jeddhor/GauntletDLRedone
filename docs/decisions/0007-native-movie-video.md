# 0007: Movie video decoded natively

Supersedes the video half of 0003-movies.md (the audio was already
decoded natively).

Since 0003 the client decoded the movies' MPEG-2 video with an FFmpeg
child process: the system's `ffmpeg` (Linux), or an `ffmpeg.exe` the player
had to put into the install folder (Windows). The project owner tested the
1.3.0 release on Windows (2026-11-05): without FFmpeg the movies did not
play at all (the owner had expected FFmpeg's code to come with the game),
and with FFmpeg installed every movie opened a console window, because the
game, a GUI program, started a console program without CREATE_NO_WINDOW.

Decision: the client decodes the video itself (`gdl_formats::mpeg2`, used
by `gdl_client::movie` on a thread of its own); no FFmpeg, no child
process, nothing to install.

- **Scope.** All 36 movies of the supported disc (MPEG/*.PSS and
  MPEG2/*.PSS) are MPEG-2 Main profile, 640×480, 30 fps, 4:2:0, progressive
  frame pictures with frame_pred_frame_dct = 1, default quantiser matrices,
  intra_vlc_format 1, linear quantiser scale, I/P/B pictures (header survey
  of every picture, research/presentation.md "Movies"). The decoder covers
  that profile (with custom matrices, both VLC formats, both scans and both
  quantiser scales) and refuses field pictures, field or dual-prime
  prediction and other chroma formats; such a stream fails to start, the
  path InitMovie's failure takes. The installer accepts only the supported
  disc, whose files are hash-checked, so no other stream reaches it.
- **Bit-exact with FFmpeg.** The inverse DCT is FFmpeg's integer "simple"
  IDCT (THIRD_PARTY.md), so the decoded pictures equal FFmpeg's: all 11 956
  pictures of the 36 movies, Linux and the Windows build under Wine, against
  FFmpeg 8.0.1's C decoder and against its default SIMD decoder
  (tools/python/movie_check.py). The pictures the player sees are therefore
  the ones the FFmpeg path gave; the frame counts are the same as the old
  command's too.
- **Tables.** The VLC tables are the standard's Tables B.1 to B.15 written
  as code lists; tools/python/mpeg2_tables_check.py compares them, for every
  16-bit pattern, with PCSX2's libmpeg2-derived lookup tables (and the scans
  and the non-linear quantiser scale as arrays): no disagreement.
- **Colour.** YCbCr → RGB now follows PCSX2's model of the IPU's colour
  space conversion (yuv2rgb_reference: BT.601 in 6-bit fixed point, one
  chroma sample per 2×2 pixels), an upstream claim about the hardware the
  original uses, where 0003 used FFmpeg's BT.601 conversion. The two differ
  by 0.7–0.8 of 255 on average (at most 19 at a few edge pixels). Neither is
  compared with a capture of the original (research/unknowns.md).

Not changed: the movie logic (init_gamemovie/do_gamemovie, the attract
sequence), the audio, the pacing (frame k at k/30 s) and what happens when a
movie file is missing. FFmpeg stays a pinned research tool (the reference of
movie_check.py; audio_compare.py), not a runtime dependency.
