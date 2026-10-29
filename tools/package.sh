#!/bin/bash
# Release packages (M21, docs/decisions/0005-installer.md).
#
#   bash tools/package.sh linux   OUT_DIR
#   GDL_WINDOWS_HOST=HOST [GDL_WINDOWS_DIR=P:/gauntlet] bash tools/package.sh windows OUT_DIR
#   WIN_INSTALLER_DIR=DIR bash tools/package.sh windows OUT_DIR
#
# Each package is a folder (and an archive of it) holding the Qt installer
# and payload/: gdl_play, gdl_import, import/ (catalog.json,
# supported-disc.json), LICENSE, THIRD_PARTY.md, THIRD_PARTY_LICENSES.txt,
# README.txt, KNOWN_ISSUES.txt. Game data is never packaged: the installer
# imports it from the user's disc. A source archive of HEAD goes beside the
# packages (the GPL's corresponding source), and qt-source/ with the source
# of the Qt that the packages carry (private/qt-src/{ubuntu,windows}: the
# LGPL's corresponding source; docs/toolchain.md). LIBRARY_LICENSES.txt in
# each package holds the bundled libraries' notices
# (tools/python/library_licenses.py).
#
# Both packages carry the installer's Qt (the user needs no Qt). Linux: the
# installer is built against the system's Qt 6 and its libraries, plugins
# and their less common dependencies are copied beside it
# (tools/bundle_qt_linux.py). Windows: the game and the importer are
# cross-built with the pinned Rust toolchain's x86_64-pc-windows-gnu target
# (docs/toolchain.md); the installer is built on a Windows machine with Qt's
# MinGW kit and deployed by windeployqt (tools/windows/build-installer.ps1),
# either over SSH (GDL_WINDOWS_HOST: an OpenSSH server with PowerShell 7,
# working folder GDL_WINDOWS_DIR) or beforehand (WIN_INSTALLER_DIR: that
# script's Out folder, its Notices folder beside it as DIR-notices).
set -euo pipefail
cd "$(dirname "$0")/.."
KIND=${1:?usage: tools/package.sh linux|windows OUT_DIR}
OUT=${2:?usage: tools/package.sh linux|windows OUT_DIR}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

payload_common() {
    local p=$1
    mkdir -p "$p/import"
    cp release/import/catalog.json release/import/supported-disc.json "$p/import/"
    cp LICENSE THIRD_PARTY.md release/THIRD_PARTY_LICENSES.txt release/README.txt release/KNOWN_ISSUES.txt "$p/"
}

qt_source() {
    mkdir -p "$OUT/qt-source"
    cp private/qt-src/"$1"/* "$OUT/qt-source/"
}

source_archive() {
    local commit
    commit=$(bash tools/git-local.sh rev-parse --short HEAD)
    bash tools/git-local.sh archive --format=tar.gz --prefix="GauntletDarkLegacy-$VERSION-source/" \
        -o "$OUT/GauntletDarkLegacy-$VERSION-source-$commit.tar.gz" HEAD
}

case "$KIND" in
linux)
    cargo build --release --locked -p gdl_client -p gdl_install --bin gdl_play --bin gdl_import
    cmake -S installer -B private/build/installer-linux -G Ninja -DCMAKE_BUILD_TYPE=Release >/dev/null
    cmake --build private/build/installer-linux
    NAME="GauntletDarkLegacy-$VERSION-linux-x86_64"
    D="$OUT/$NAME"
    rm -rf "$D"
    mkdir -p "$D/payload"
    cp private/build/installer-linux/GauntletDarkLegacy-Installer "$D/"
    cp target/release/gdl_play target/release/gdl_import "$D/payload/"
    strip "$D/payload/gdl_play" "$D/payload/gdl_import" "$D/GauntletDarkLegacy-Installer"
    python3 tools/python/bundle_qt_linux.py "$D"
    python3 tools/python/library_licenses.py linux "$D" private/qt-src/ubuntu "$D/LIBRARY_LICENSES.txt"
    payload_common "$D/payload"
    cp release/README.txt LICENSE "$D/"
    qt_source ubuntu
    tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
    ;;
windows)
    TC=~/.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin
    RUSTC=$TC/rustc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        CARGO_TARGET_DIR=target-win "$TC/cargo" build --release --offline --locked \
        --target x86_64-pc-windows-gnu -p gdl_client -p gdl_install --bin gdl_play --bin gdl_import
    INST=${WIN_INSTALLER_DIR:-}
    if [ -z "$INST" ]; then
        : "${GDL_WINDOWS_HOST:?set GDL_WINDOWS_HOST (build the installer there over SSH) or WIN_INSTALLER_DIR}"
        R=${GDL_WINDOWS_DIR:-P:/gauntlet}
        ssh -o BatchMode=yes "$GDL_WINDOWS_HOST" pwsh -NoProfile -Command \
            "\"Remove-Item -Recurse -Force -ErrorAction SilentlyContinue '$R/pkg-src'; New-Item -ItemType Directory '$R/pkg-src' | Out-Null\""
        scp -q -r -o BatchMode=yes installer tools/windows/build-installer.ps1 "$GDL_WINDOWS_HOST:$R/pkg-src/"
        ssh -o BatchMode=yes "$GDL_WINDOWS_HOST" pwsh -NoProfile -File "$R/pkg-src/build-installer.ps1" \
            -Source "$R/pkg-src/installer" -Build "$R/pkg-build" -Out "$R/pkg-deploy" -Notices "$R/pkg-notices"
        INST=private/build/installer-windows-deploy
        rm -rf "$INST" "$INST-notices"
        scp -q -r -o BatchMode=yes "$GDL_WINDOWS_HOST:$R/pkg-deploy" "$INST"
        scp -q -r -o BatchMode=yes "$GDL_WINDOWS_HOST:$R/pkg-notices" "$INST-notices"
    fi
    # The deployed files as the Windows side listed them (SHA-256, size, path).
    (cd "$INST" && tr -d '\r' < deployed-files.txt | awk '{print $1 "  " $3}' | sha256sum --quiet -c -)
    NAME="GauntletDarkLegacy-$VERSION-windows-x86_64"
    D="$OUT/$NAME"
    rm -rf "$D"
    mkdir -p "$D/payload"
    (cd "$INST" && tr -d '\r' < deployed-files.txt | awk '{print $3}') | while read -r f; do
        mkdir -p "$D/$(dirname "$f")"
        cp "$INST/$f" "$D/$f"
    done
    # Stripped at link time (.cargo/config.toml; strip would add a time stamp).
    cp target-win/x86_64-pc-windows-gnu/release/gdl_play.exe target-win/x86_64-pc-windows-gnu/release/gdl_import.exe "$D/payload/"
    payload_common "$D/payload"
    cp release/README.txt LICENSE "$D/"
    python3 tools/python/library_licenses.py windows "$INST" "$INST-notices" \
        private/qt-src/windows/qtbase-everywhere-src-*.tar.xz private/qt-src/windows "$D/LIBRARY_LICENSES.txt"
    cp "$D/LIBRARY_LICENSES.txt" "$D/payload/"
    qt_source windows
    (cd "$OUT" && rm -f "$NAME.zip" && 7z a -tzip -bd "$NAME.zip" "$NAME" >/dev/null)
    ;;
*)
    echo "usage: tools/package.sh linux|windows OUT_DIR" >&2
    exit 2
    ;;
esac
source_archive
(cd "$OUT" && sha256sum ./*.tar.gz ./*.zip ./qt-source/* 2>/dev/null > SHA256SUMS || true)
ls -la "$OUT"
