#!/usr/bin/env python3
"""Notices for the native libraries a release package carries (M21).

  library_licenses.py linux PACKAGE_DIR QT_SBOM QT_SOURCE_TAR SOURCE_DIR OUT.txt
  library_licenses.py windows DEPLOY_DIR NOTICES_DIR QT_SOURCE_TAR SOURCE_DIR OUT.txt

The Rust code of gdl_play and gdl_import is covered by
THIRD_PARTY_LICENSES.txt (third_party_licenses.py); this covers the rest:

linux    the files tools/python/bundle_qt_linux.py copied
         (PACKAGE_DIR/bundled-libraries.txt): Qt 6 as built by
         tools/linux/build-qt.sh, with the third-party components its SBOM
         (QT_SBOM, tag-value) records inside the bundled files, their
         copyright texts and their license texts from Qt's source
         (QT_SOURCE_TAR, LICENSES/); the other libraries Qt needs, copied
         from Ubuntu 20.04's binary packages, with each package's Debian
         copyright file verbatim and the /usr/share/common-licenses texts
         those files refer to (run inside the build container).
windows  the files tools/windows/build-installer.ps1 deployed
         (DEPLOY_DIR/deployed-files.txt): Qt 6's libraries and plugins,
         with the third-party components Qt's SBOM
         (NOTICES_DIR/qtbase-*.spdx.json) records inside them, their
         copyright texts and their license texts from Qt's own source
         (QT_SOURCE_TAR, LICENSES/); the MinGW-w64 runtime DLLs (the Qt
         kit's license files in NOTICES_DIR/mingw; their GCC source in
         SOURCE_DIR); and the MinGW-w64 startup code linked into the
         cross-built .exe files (Ubuntu's mingw-w64 copyright file).

SOURCE_DIR: the source archives shipped beside the packages (the folder
"library-source"; names and SHA-256 are listed). Offline; reads only local
files.
"""
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tarfile

RULE = '=' * 78


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def sources_block(source_dir, qt):
    """SOURCE_DIR's Qt archives (qt) or its other files (not qt), listed."""
    files = sorted(p for p in pathlib.Path(source_dir).iterdir() if p.is_file() and p.name.startswith('qt') == qt)
    if not files:
        raise SystemExit(f'{source_dir}: no {"Qt" if qt else "other"} source archives')
    return [f'  {sha256(p)}  {p.name}' for p in files]


def lgpl_statement(qt, how, sources):
    return [
        f'Qt {qt} is (C) The Qt Company Ltd and other contributors. It is offered under',
        'LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only; this package',
        'uses it under the GNU Lesser General Public License version 3 (LGPL-3.0-only,',
        'text below; it adds permissions to the GNU General Public License version 3 in',
        'LICENSE). ' + how,
        'The libraries are dynamically linked and unmodified: you may replace them with',
        f'your own build of Qt {qt}. Their complete corresponding source is distributed',
        'beside this package (the folder "library-source" of the release):',
        *sources,
    ]


# ------------------------------------------------------------------ qt sbom

QT_OWN = 'LicenseRef-Qt-Commercial OR LGPL-3.0-only OR GPL-2.0-only OR GPL-3.0-only'


def chosen(expr):
    """The license IDs a component is used under (one alternative of an OR)."""
    if expr == QT_OWN:
        return ['LGPL-3.0-only']
    alts = [a.strip() for a in expr.split(' OR ')]
    pick = next((a for a in alts if not a.startswith(('LicenseRef-Qt-Commercial', 'GPL-'))), alts[0])
    return [t.strip() for t in pick.split(' AND ')]


TAGS = {'SPDXID': 'SPDXID', 'PackageVersion': 'versionInfo', 'PackageLicenseConcluded': 'licenseConcluded',
        'PackageLicenseDeclared': 'licenseDeclared', 'PackageCopyrightText': 'copyrightText'}


def load_sbom(path):
    """An SPDX 2.3 document as the JSON form's fields: a .json file, or the
    tag-value form a Qt build writes (sbom/qtbase-VERSION.spdx)."""
    text = pathlib.Path(path).read_text()
    if str(path).endswith('.json'):
        return json.loads(text)
    doc, cur, lines, i = {'packages': [], 'relationships': []}, None, text.splitlines(), 0
    while i < len(lines):
        line = lines[i]
        i += 1
        if ':' not in line:
            continue
        tag, value = (part.strip() for part in line.split(':', 1))
        if value.startswith('<text>'):
            value = value[len('<text>'):]
            while '</text>' not in value:
                value += '\n' + lines[i]
                i += 1
            value = value[:value.index('</text>')]
        if tag == 'DocumentName':
            doc['name'] = value
        elif tag == 'PackageName':
            cur = {'name': value}
            doc['packages'].append(cur)
        elif tag in ('FileName', 'SnippetSPDXID', 'LicenseID'):
            cur = None
        elif tag == 'Relationship':
            a, kind, b = value.split()[:3]
            doc['relationships'].append({'spdxElementId': a, 'relationshipType': kind, 'relatedSpdxElement': b})
        elif cur is not None and tag in TAGS:
            cur[TAGS[tag]] = value
    return doc


def qt_notices(sbom, sbom_name, files):
    """Lines for the bundled Qt files ({file: SBOM package name}): each file's
    license and copyright, then the third-party components inside them.
    Returns (lines, the license IDs used)."""
    pk = {p['SPDXID']: p for p in sbom['packages']}
    byname = {p['name']: p for p in sbom['packages']}
    rel = {}
    for r in sbom['relationships']:
        if r['relationshipType'] in ('DEPENDS_ON', 'CONTAINS'):
            rel.setdefault(r['spdxElementId'], []).append(r['relatedSpdxElement'])

    def components(root):
        found, todo, seen = [], [byname[root]['SPDXID']], set()
        while todo:
            sid = todo.pop(0)
            if sid in seen or sid not in pk:
                continue
            seen.add(sid)
            p = pk[sid]
            if sid != byname[root]['SPDXID'] and ('_Attribution_' in p['name'] or p['name'].startswith('Bundled')):
                found.append(p)
            if sid == byname[root]['SPDXID'] or p['name'].startswith(('Bundled', 'Platform')):
                todo += rel.get(sid, [])
        return found

    ids, out = {'LGPL-3.0-only'}, []
    for f, name in files.items():
        p = byname[name]
        out += [f'{f} (Qt {name}): {p.get("licenseDeclared", "NOASSERTION")}', '  ' + p.get('copyrightText', '').replace('\n', '\n  '), '']
    out += ['', RULE, f'THIRD-PARTY COMPONENTS INSIDE THOSE QT FILES ({sbom_name})', RULE]
    listed = set()
    for f, name in files.items():
        for c in components(name):
            if c['SPDXID'] in listed:
                continue
            listed.add(c['SPDXID'])
            expr = c.get('licenseConcluded') or c.get('licenseDeclared')
            use = chosen(expr)
            ids.update(use)
            label = c['name'].split('_Attribution_')[-1] if '_Attribution_' in c['name'] else c['name'].replace('Bundled', '')
            out += ['', f'{label} {c.get("versionInfo", "")} (in {f}) — {expr}' + (f'; used under {" AND ".join(use)}' if ' OR ' in expr else ''),
                    '  ' + (c.get('copyrightText') or 'NOASSERTION').replace('\n', '\n  ')]
    return out, ids, len(listed)


def license_texts(qt_tar, ids):
    out = ['', RULE, 'LICENSE TEXTS (FROM QT\'S SOURCE, LICENSES/)', RULE]
    with tarfile.open(qt_tar) as t:
        members = {pathlib.PurePosixPath(m.name).name: m for m in t.getmembers()
                   if re.match(r'^[^/]+/LICENSES/[^/]+\.txt$', m.name)}
        for i in sorted(ids):
            m = members.get(f'{i}.txt')
            if m is None:
                raise SystemExit(f'{qt_tar}: no LICENSES/{i}.txt')
            out += ['', '-' * 78, i, '-' * 78, '', t.extractfile(m).read().decode('utf-8', 'replace').rstrip()]
    return out


# --------------------------------------------------------------------- linux

QT_LINUX_FILES = {
    'lib/libQt6Core.so.6': 'Core', 'lib/libQt6Gui.so.6': 'Gui', 'lib/libQt6Widgets.so.6': 'Widgets',
    'lib/libQt6DBus.so.6': 'DBus', 'lib/libQt6XcbQpa.so.6': 'XcbQpaPrivate',
    'plugins/platforms/libqxcb.so': 'QXcbIntegrationPlugin',
    'plugins/platforminputcontexts/libcomposeplatforminputcontextplugin.so': 'QComposePlatformInputContextPlugin',
    'plugins/platforminputcontexts/libibusplatforminputcontextplugin.so': 'QIbusPlatformInputContextPlugin',
}


def linux(package, sbom_path, qt_tar, source_dir, out_path):
    rows = [line.split() for line in (package / 'bundled-libraries.txt').read_text().splitlines() if line.strip()]
    qt_rows = [r for r in rows if r[5] == 'qtbase']
    unknown = [r[2] for r in qt_rows if r[2] not in QT_LINUX_FILES]
    if unknown:
        raise SystemExit(f'bundled Qt files without a notice mapping: {unknown}')
    by_source = {}
    for digest, size, path, pkg, ver, src, srcver in rows:
        if src != 'qtbase':
            by_source.setdefault((src, srcver), []).append((path, pkg, ver, digest, size))
    sbom = load_sbom(sbom_path)
    qt = qt_rows[0][6]
    out = [RULE, 'LIBRARIES BUNDLED WITH THE INSTALLER (LINUX PACKAGE)', RULE, '',
           'GauntletDarkLegacy-Installer is a Qt 6 program. So that it runs without Qt being',
           'installed, this package carries Qt and the less common libraries Qt needs, in',
           'lib/ and plugins/ beside it (qt.conf points Qt there). Qt is built from its source',
           '(tools/linux/build-qt.sh in the source archive); the other libraries are unmodified',
           'copies of Ubuntu 20.04 binary packages, each package\'s Debian copyright file',
           'follows. The game (payload/gdl_play) and the importer (payload/gdl_import) use none',
           'of them; their own notices are in payload/THIRD_PARTY_LICENSES.txt.', '',
           'FILES (SHA-256, size, path, origin)', '']
    for digest, size, path, pkg, ver, src, _ in rows:
        origin = f'Qt {ver}, built' if src == 'qtbase' else f'{pkg} {ver}'
        out.append(f'  {digest}  {int(size):>9}  {path}  ({origin})')
    files = {r[2]: QT_LINUX_FILES[r[2]] for r in qt_rows}
    notices, ids, n = qt_notices(sbom, pathlib.Path(sbom_path).name, files)
    out += ['', RULE, f'QT {qt}', RULE, '',
            *lgpl_statement(qt, f'The files are built from qtbase {qt} with GCC 10 on Ubuntu 20.04 by tools/linux/build-qt.sh.',
                            sources_block(source_dir, True)), '', *notices]
    texts, common = {}, set()
    out += ['', RULE, 'DEBIAN COPYRIGHT FILES', RULE]
    for (src, srcver), files_ in sorted(by_source.items()):
        for pkg in sorted({f[1] for f in files_}):
            text = pathlib.Path(f'/usr/share/doc/{pkg}/copyright').read_text(errors='replace')
            texts.setdefault(text, []).append(pkg)
            common.update(re.findall(r'/usr/share/common-licenses/([A-Za-z0-9.+-]*[A-Za-z0-9+])', text))
    for text, pkgs in texts.items():
        out += ['', '-' * 78, f'{", ".join(pkgs)} (/usr/share/doc/{pkgs[0]}/copyright)', '-' * 78, '', text.rstrip()]
    out += ['', RULE, 'LICENSE TEXTS REFERRED TO ABOVE (/usr/share/common-licenses)', RULE]
    for name in sorted(common):
        p = pathlib.Path('/usr/share/common-licenses') / name
        if p.is_file():
            out += ['', '-' * 78, name, '-' * 78, '', p.read_text(errors='replace').rstrip()]
    out += license_texts(qt_tar, ids)
    pathlib.Path(out_path).write_text('\n'.join(out) + '\n')
    print(f'{out_path}: {len(rows)} files, {n} Qt components, {len(texts)} copyright files, '
          f'common licenses {sorted(common)}, Qt licenses {sorted(ids)}')


# ------------------------------------------------------------------- windows

QT_FILES = {
    'Qt6Core.dll': 'Core', 'Qt6Gui.dll': 'Gui', 'Qt6Widgets.dll': 'Widgets',
    'platforms/qwindows.dll': 'QWindowsIntegrationPlugin', 'styles/qmodernwindowsstyle.dll': 'QModernWindowsStylePlugin',
}
MINGW_FILES = {
    'libgcc_s_seh-1.dll': 'GCC runtime library (libgcc)',
    'libstdc++-6.dll': 'GNU C++ standard library (libstdc++)',
    'libwinpthread-1.dll': 'MinGW-w64 winpthreads',
}


def windows(deploy, notices, qt_tar, source_dir, out_path):
    deployed = [line.split() for line in (deploy / 'deployed-files.txt').read_text(encoding='utf-8-sig').splitlines() if line.strip()]
    sbom_path = next(notices.glob('qtbase-*.spdx.json'))
    sbom = load_sbom(sbom_path)
    qt = sbom['name'].split('-')[-1] if sbom['name'][-1].isdigit() else next(
        p['versionInfo'] for p in sbom['packages'] if p['name'] == 'Core')
    unknown = [p for _, _, p in deployed if p != 'GauntletDarkLegacy-Installer.exe' and p not in QT_FILES and p not in MINGW_FILES]
    if unknown:
        raise SystemExit(f'deployed files without a notice mapping: {unknown}')
    out = [RULE, 'LIBRARIES BUNDLED WITH THE INSTALLER (WINDOWS PACKAGE)', RULE, '',
           'GauntletDarkLegacy-Installer.exe is a Qt 6 program; Qt\'s libraries and plugins and',
           'the MinGW-w64 runtime they need are beside it. The game (payload/gdl_play.exe) and',
           'the importer (payload/gdl_import.exe) use none of them; their Rust code\'s notices',
           'are in payload/THIRD_PARTY_LICENSES.txt, and the MinGW-w64 startup code linked into',
           'them is covered at the end of this file.', '',
           'FILES (SHA-256, size, path)', '']
    for digest, size, path in deployed:
        out.append(f'  {digest}  {int(size):>9}  {path}')
    notices_, ids, n = qt_notices(sbom, sbom_path.name, QT_FILES)
    out += ['', RULE, f'QT {qt}', RULE, '',
            *lgpl_statement(qt, f'The files are The Qt Company\'s official Qt {qt} binaries for MinGW 13.1 (64-bit).',
                            sources_block(source_dir, True)), '', *notices_]
    mingw = notices / 'mingw'
    out += ['', RULE, 'MINGW-W64 RUNTIME (THE QT KIT\'S MINGW 13.1)', RULE, '']
    for f, what in MINGW_FILES.items():
        out.append(f'{f}: {what}')
    out += ['',
            'libgcc_s_seh-1.dll and libstdc++-6.dll are part of GCC 13.1.0 as built by the',
            'MinGW-Builds project (MinGW-W64-builds-5.0.0, x86_64-posix-seh, msvcrt, runtime v11,',
            'revision 1), licensed under the GNU General Public License version 3 with the GCC',
            'Runtime Library Exception version 3.1 (both texts below). Their complete',
            'corresponding source is distributed beside this package (the folder',
            '"library-source" of the release): GCC 13.1.0\'s release archive, the MinGW-Builds',
            'scripts and patches the build used (niXman/mingw-builds at commit 7ff96692, the',
            'project\'s development branch when that release was built; scripts/gcc-13.1.0.sh',
            'and patches/gcc/), and the build\'s own record of its sources, patches and',
            'configuration (the kit\'s build-info.txt):',
            *sources_block(source_dir, False), '',
            'libwinpthread-1.dll is MinGW-w64\'s winpthreads (notice below).', '']
    for title, rel_path in [('GCC RUNTIME LIBRARY EXCEPTION 3.1 (gcc/COPYING.RUNTIME)', 'gcc/COPYING.RUNTIME'),
                            ('GNU GENERAL PUBLIC LICENSE VERSION 3 (gcc/COPYING3)', 'gcc/COPYING3'),
                            ('WINPTHREADS (winpthreads/COPYING)', 'winpthreads/COPYING'),
                            ('MINGW-W64 RUNTIME (mingw-w64/COPYING.MinGW-w64-runtime.txt)', 'mingw-w64/COPYING.MinGW-w64-runtime.txt')]:
        out += ['-' * 78, title, '-' * 78, '', (mingw / rel_path).read_text(errors='replace').rstrip(), '']
    ver = subprocess.run(['dpkg-query', '-W', '-f', '${Version}', 'mingw-w64-common'], capture_output=True, text=True).stdout
    out += [RULE, f'MINGW-W64 STARTUP CODE IN gdl_play.exe AND gdl_import.exe (Ubuntu mingw-w64 {ver})', RULE, '',
            'The game and the importer are cross-built with Ubuntu\'s MinGW-w64 toolchain, whose',
            'C runtime startup code and import libraries are linked into them. Ubuntu\'s',
            'copyright file for mingw-w64 (/usr/share/doc/mingw-w64-common/copyright):', '',
            pathlib.Path('/usr/share/doc/mingw-w64-common/copyright').read_text(errors='replace').rstrip()]
    out += license_texts(qt_tar, ids)
    pathlib.Path(out_path).write_text('\n'.join(out) + '\n')
    print(f'{out_path}: {len(deployed)} files, {n} Qt components, licenses {sorted(ids)}')


def main():
    a = sys.argv[1:]
    if len(a) == 6 and a[0] == 'linux':
        linux(pathlib.Path(a[1]), a[2], a[3], a[4], a[5])
    elif len(a) == 6 and a[0] == 'windows':
        windows(pathlib.Path(a[1]), pathlib.Path(a[2]), a[3], a[4], a[5])
    else:
        raise SystemExit(__doc__)


if __name__ == '__main__':
    main()
