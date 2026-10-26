"""The reference's render window on the virtual display (research only).

Screenshots capture the root window, so the render window must be
uncovered. Xvfb has no window manager: when another window overlaps it
(e.g. a gdl_play window left on the display), the reference's own top-level
windows are raised and each must then lie above every overlapping window in
the root's stacking order (`xwininfo -root -children` lists the top-most
first). The other window is never touched.
"""
import subprocess


def raise_render(env, xdo, windows, render, verify=True):
    """Raise the reference (its top-level windows, then the render window
    `render`); with `verify`, refuse when another visible window still
    covers the render window."""
    def rect(w):
        g = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', w).splitlines() if '=' in line)
        return int(g['X']), int(g['Y']), int(g['X']) + int(g['WIDTH']), int(g['Y']) + int(g['HEIGHT'])

    def stacking():
        out = subprocess.run(['xwininfo', '-root', '-children'], env=env, check=True, capture_output=True, text=True, timeout=5).stdout
        return [int(line.split()[0], 16) for line in out.splitlines() if line.strip().startswith('0x')]

    rx0, ry0, rx1, ry1 = rect(render)
    covering = []
    for w in xdo('search', '--onlyvisible', '--name', '.').split():
        if w in windows or not w.isdigit():
            continue
        x0, y0, x1, y1 = rect(w)
        if x0 < rx1 and rx0 < x1 and y0 < ry1 and ry0 < y1 and 1 < x1 - x0 and 1 < y1 - y0:
            covering.append(int(w))
    own = [w for w in stacking() if str(w) in windows]
    for w in reversed(own):
        xdo('windowraise', str(w))
    xdo('windowraise', render)
    if verify and covering:
        order = stacking()
        top = [order.index(w) for w in own if w in order]
        for w in covering:
            if w in order and (not top or order.index(w) < max(top)):
                raise ValueError(f'window {w} ({xdo("getwindowname", str(w)).strip()}) stays above the reference render window')
