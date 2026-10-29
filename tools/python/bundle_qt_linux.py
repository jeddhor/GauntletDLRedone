#!/usr/bin/env python3
"""Copy the installer's Qt beside it for the Linux package (M21).

  bundle_qt_linux.py PACKAGE_DIR

PACKAGE_DIR holds GauntletDarkLegacy-Installer, linked against the system's
Qt 6 with DT_RPATH $ORIGIN/lib (installer/CMakeLists.txt). This adds:

  lib/       Qt's libraries and the less common libraries they need
  plugins/   the platform plugins (X11 and Wayland), Wayland's xdg-shell and
             decoration plugins, the compose and IBus input methods
  qt.conf    Qt's prefix: the package folder, so only these plugins load
  bundled-libraries.txt  per file: SHA-256, size, path, Debian package and
             version, source package and version (read by library_licenses.py)

The libraries are the ldd closure of the installer and the plugins, less
the base system (KEEP_SYSTEM: the C library, the compiler runtime, the
graphics drivers' libraries, core X11/Wayland client libraries, fonts,
GLib, D-Bus, systemd and compression libraries), which every desktop has
and which must match the running system. Offline; reads only local files.
"""
import hashlib
import pathlib
import re
import shutil
import subprocess
import sys

QT_PLUGINS = pathlib.Path('/usr/lib/x86_64-linux-gnu/qt6/plugins')
PLUGINS = [
    'platforms/libqxcb.so',
    'platforms/libqwayland.so',
    'wayland-shell-integration/libxdg-shell.so',
    'wayland-decoration-client/libbradient.so',
    'platforminputcontexts/libcomposeplatforminputcontextplugin.so',
    'platforminputcontexts/libibusplatforminputcontextplugin.so',
]
KEEP_SYSTEM = re.compile(r'^(' + '|'.join([
    r'ld-linux-x86-64\.so\.2', r'libc\.so\.6', r'libm\.so\.6', r'libdl\.so\.2', r'libpthread\.so\.0', r'librt\.so\.1',
    r'libresolv\.so\.2', r'libutil\.so\.1',
    r'libstdc\+\+\.so\.6', r'libgcc_s\.so\.1', r'libgomp\.so\.1', r'libatomic\.so\.1',
    r'libEGL\.so\.1', r'libGL\.so\.1', r'libGLX\.so\.0', r'libGLdispatch\.so\.0', r'libOpenGL\.so\.0', r'libdrm\.so\.2',
    r'libgbm\.so\.1', r'libvulkan\.so\.1',
    r'libX11\.so\.6', r'libX11-xcb\.so\.1', r'libxcb\.so\.1', r'libXau\.so\.6', r'libXdmcp\.so\.6', r'libXext\.so\.6',
    r'libwayland-client\.so\.0', r'libwayland-egl\.so\.1', r'libwayland-server\.so\.0',
    r'libfontconfig\.so\.1', r'libfreetype\.so\.6', r'libharfbuzz\.so\.0', r'libgraphite2\.so\.3', r'libpng16\.so\.16',
    r'libexpat\.so\.1', r'libglib-2\.0\.so\.0', r'libgobject-2\.0\.so\.0', r'libgio-2\.0\.so\.0', r'libgmodule-2\.0\.so\.0',
    r'libgthread-2\.0\.so\.0', r'libpcre2-8\.so\.0', r'libffi\.so\.8', r'libmount\.so\.1', r'libblkid\.so\.1',
    r'libselinux\.so\.1', r'libuuid\.so\.1', r'libdbus-1\.so\.3', r'libsystemd\.so\.0', r'libcap\.so\.2',
    r'libz\.so\.1', r'libzstd\.so\.1', r'libbz2\.so\.1\.0', r'libbrotli(common|dec)\.so\.1', r'liblzma\.so\.5', r'liblz4\.so\.1',
]) + r')$')


def ldd(path):
    out = subprocess.run(['ldd', str(path)], capture_output=True, text=True, check=True).stdout
    deps = {}
    for line in out.splitlines():
        m = re.match(r'\s*(\S+) => (\S+) \(', line)
        if m:
            deps[m.group(1)] = pathlib.Path(m.group(2))
        elif 'not found' in line:
            raise SystemExit(f'{path}: {line.strip()}')
    return deps


def debian_package(path):
    r = subprocess.run(['dpkg', '-S', str(path.resolve())], capture_output=True, text=True)
    if r.returncode != 0:
        r = subprocess.run(['dpkg', '-S', str(path)], capture_output=True, text=True, check=True)
    pkg = r.stdout.split(':', 1)[0].strip()
    q = subprocess.run(['dpkg-query', '-W', '-f', '${Package} ${Version} ${source:Package} ${source:Version}\n', pkg],
                       capture_output=True, text=True, check=True).stdout
    return q.splitlines()[0].split()  # one line per installed architecture, all the same version


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    pkg = pathlib.Path(sys.argv[1]).resolve()
    exe = pkg / 'GauntletDarkLegacy-Installer'
    rpath = subprocess.run(['readelf', '-d', str(exe)], capture_output=True, text=True, check=True).stdout
    if not re.search(r'\(RPATH\)\s+Library rpath: \[\$ORIGIN/lib\]', rpath):
        raise SystemExit(f'{exe}: no DT_RPATH $ORIGIN/lib (installer/CMakeLists.txt)')
    for d in ('lib', 'plugins'):
        shutil.rmtree(pkg / d, ignore_errors=True)
    files = []
    needed = dict(ldd(exe))
    for rel in PLUGINS:
        src = QT_PLUGINS / rel
        dst = pkg / 'plugins' / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        files.append((dst, src))
        needed.update(ldd(src))
    for name, src in sorted(needed.items()):
        if KEEP_SYSTEM.match(name):
            continue
        dst = pkg / 'lib' / name
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src.resolve(), dst)
        files.append((dst, src))
    (pkg / 'qt.conf').write_text('[Paths]\nPrefix = .\nLibraries = lib\nPlugins = plugins\n')
    rows = []
    for dst, src in sorted(files, key=lambda f: str(f[0])):
        data = dst.read_bytes()
        rows.append(' '.join([hashlib.sha256(data).hexdigest(), str(len(data)), str(dst.relative_to(pkg))] + debian_package(src)))
    (pkg / 'bundled-libraries.txt').write_text('\n'.join(rows) + '\n')
    kept = sorted({n for n in needed if KEEP_SYSTEM.match(n)})
    print(f'{len(rows)} files bundled ({sum(f[0].stat().st_size for f in files) / 2**20:.1f} MiB); '
          f'left to the system: {" ".join(kept)}')


if __name__ == '__main__':
    main()
