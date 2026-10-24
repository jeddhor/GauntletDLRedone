#!/usr/bin/env python3
"""Compare a reference audio capture with the native decode of a music
stream (M17 research; not a fidelity gate on its own).

  audio_compare.py CAPTURE.mp4 STREAM NEW_REPORT.json [--from S] [--seconds S]

CAPTURE is a PCSX2 video capture (its audio: the SPU2 output in emulated
time, AAC 48 kHz stereo — lossy, so only levels, timing and the spectrum
below ~15 kHz are meaningful). STREAM names an ADS file (e.g. TOWN1); its
PCM comes from the native decoder (gdl_tools ads-pcm). A window of the
capture is located in the stream by FFT cross-correlation of the mono
signals; then per channel the least-squares gain g (capture ≈ g × native
PCM / 32768), the correlation coefficient, the residual level, and the
capture/native magnitude ratio in third-octave bands. AdsSetVolume's word
v gives the native stream gain ((v × 0x1fff) >> 8) / 0x4000; the report
also gives the word whose gain is nearest the fitted one.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys

import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[2]
TOOLS = os.environ.get('GDL_TOOLS', str(ROOT / 'target/release/gdl_tools'))


def stream_file(name):
    names = json.loads((ROOT / 'private/reports/wad-independent.json').read_text())['entries']
    man = json.loads((ROOT / 'private/wad/manifest.json').read_text())
    for x in names:
        for n in x['proposed_names']:
            if n.upper() == f'STREAMS/{name.upper()}.ADS':
                return ROOT / 'private/wad' / man['entries'][x['index']]['output_file']
    raise SystemExit(f'no stream {name}')


def capture_pcm(path, limit_s=600):
    out = subprocess.run(['ffmpeg', '-v', 'error', '-i', str(path), '-vn', '-f', 'f32le', '-ac', '2', '-ar', '48000', '-t', str(limit_s), '-'],
                         capture_output=True, check=True).stdout
    return np.frombuffer(out, '<f4').reshape(-1, 2).astype(np.float64)


def native_pcm(stream, scratch):
    raw = scratch / f'ads-{stream.name}.raw'
    if raw.exists():
        raw.unlink()
    info = subprocess.run([TOOLS, 'ads-pcm', str(stream), str(raw)], capture_output=True, text=True, check=True).stdout.split()
    rate, ch, frames = (int(v) for v in info)
    pcm = np.fromfile(raw, '<i2').astype(np.float64).reshape(-1, ch) / 32768.0
    raw.unlink()
    return rate, pcm


def xcorr_offset(long, short):
    n = 1 << int(np.ceil(np.log2(len(long) + len(short))))
    c = np.fft.irfft(np.fft.rfft(long, n) * np.conj(np.fft.rfft(short, n)), n)[:len(long) - len(short) + 1]
    k = int(np.argmax(c))
    return k, c[k]


def gain_word(g):
    best = min(range(0, 0x100), key=lambda v: abs(((v * 0x1fff) >> 8) / 16384.0 - g))
    return best, ((best * 0x1fff) >> 8) / 16384.0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('capture', type=pathlib.Path)
    ap.add_argument('stream')
    ap.add_argument('report', type=pathlib.Path)
    ap.add_argument('--from', dest='start', type=float, default=10.0)
    ap.add_argument('--seconds', type=float, default=8.0)
    a = ap.parse_args()
    if a.report.exists() or not a.report.resolve().is_relative_to((ROOT / 'private').resolve()):
        raise SystemExit('choose a new report under private/')
    scratch = a.report.parent
    cap = capture_pcm(a.capture)
    rate, nat = native_pcm(stream_file(a.stream), scratch)
    if rate != 48000:
        # Linear resampling of the native stream to the capture's 48 kHz
        # (only for locating and levels; the spectrum is then approximate).
        t = np.arange(int(len(nat) * 48000 / rate)) * rate / 48000
        nat = np.stack([np.interp(t, np.arange(len(nat)), nat[:, c]) for c in range(nat.shape[1])], 1)
    if nat.shape[1] == 1:
        nat = np.repeat(nat, 2, 1)
    s0, n = int(a.start * 48000), int(a.seconds * 48000)
    win = cap[s0:s0 + n]
    if len(win) < n:
        raise SystemExit('capture shorter than the window')
    # Locate in the stream played twice (a loop may wrap inside the window).
    loop = np.concatenate([nat, nat])
    off, _ = xcorr_offset(loop.mean(1), win.mean(1))
    ref = loop[off:off + n]
    out = dict(capture=str(a.capture), stream=a.stream, stream_rate=rate, stream_seconds=round(len(nat) / 48000, 3),
               window=[a.start, a.seconds], offset_in_stream_s=round(off / 48000, 5), channels=[],
               scope='M17 research: PCSX2 capture (AAC, lossy) against the native ADS decode; levels and spectrum, not samples')
    edges = 1000 * 2.0 ** (np.arange(-15, 13) / 3)
    for c in range(2):
        x, y = win[:, c], ref[:, c]
        g = float(np.dot(x, y) / max(np.dot(y, y), 1e-30))
        r = float(np.corrcoef(x, y)[0, 1])
        resid = x - g * y
        X, Y = np.abs(np.fft.rfft(x * np.hanning(n))), np.abs(np.fft.rfft(g * y * np.hanning(n)))
        f = np.fft.rfftfreq(n, 1 / 48000)
        bands = []
        for lo, hi in zip(edges[:-1], edges[1:]):
            m = (lo <= f) & (f < hi)
            if m.any() and hi <= 20000:
                bands.append([round(float(np.sqrt(lo * hi))), round(float(20 * np.log10(np.sqrt((X[m] ** 2).sum() / max((Y[m] ** 2).sum(), 1e-30)))), 2)])
        word, wg = gain_word(g)
        out['channels'].append(dict(gain=round(g, 5), corr=round(r, 5), residual_db=round(float(20 * np.log10(np.sqrt(np.mean(resid ** 2)) / max(np.sqrt(np.mean(x ** 2)), 1e-30))), 2),
                                    rms_capture=round(float(np.sqrt(np.mean(x ** 2))), 5), nearest_volume_word=word, its_gain=round(wg, 5),
                                    band_ratio_db=bands))
    a.report.write_text(json.dumps(out, indent=1) + '\n')
    for c in out['channels']:
        print({k: v for k, v in c.items() if k != 'band_ratio_db'})
        print(' bands', c['band_ratio_db'])
    print('offset', out['offset_in_stream_s'])


if __name__ == '__main__':
    main()
