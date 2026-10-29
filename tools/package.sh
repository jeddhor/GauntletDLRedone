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
# packages (the GPL's corresponding source), and library-source/ with the
# source of the libraries the packages carry (private/library-src/{linux,
# windows}: Qt's, and for Windows the MinGW-w64 GCC runtime's; the LGPL's
# and GPL's corresponding source; docs/toolchain.md). LIBRARY_LICENSES.txt
# in each package holds the bundled libraries' notices
# (tools/python/library_licenses.py).
#
# Both packages carry the installer's Qt (the user needs no Qt). Linux:
# everything is built in an Ubuntu 20.04 container (tools/linux/: glibc
# 2.31, so the package runs on distributions from 2020 on) with the pinned
# Rust toolchain and the offline registry mounted; Qt is built from its
# source there once (private/build/qt-6.10.2-focal) and copied beside the
# installer with its less common dependencies (bundle_qt_linux.py); the
# build tree is GDL_LINUX_WORK (default /tmp/gdl-linux-build). Windows: the game and the importer are
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

library_source() {
    mkdir -p "$OUT/library-source"
    cp private/library-src/"$1"/* "$OUT/library-source/"
}

source_archive() {
    local commit
    commit=$(bash tools/git-local.sh rev-parse --short HEAD)
    bash tools/git-local.sh archive --format=tar.gz --prefix="GauntletDarkLegacy-$VERSION-source/" \
        -o "$OUT/GauntletDarkLegacy-$VERSION-source-$commit.tar.gz" HEAD
}

case "$KIND" in
linux)
    IMG=gdl-build-focal:2
    podman image exists "$IMG" || podman build -t "$IMG" -f tools/linux/Containerfile tools/linux
    QT=private/build/qt-6.10.2-focal
    WORK=${GDL_LINUX_WORK:-/tmp/gdl-linux-build}
    mkdir -p "$WORK" "$QT"
    RUN=(podman run --rm --network=none --userns=keep-id -v "$PWD:/src:ro" -v "$WORK:/work" -w /src)
    if [ ! -e "$QT/lib/libQt6Widgets.so.6" ]; then
        "${RUN[@]}" -v "$PWD/$QT:/opt/qt" "$IMG" bash tools/linux/build-qt.sh \
            /src/private/library-src/linux/qt6-base_6.10.2+dfsg.orig.tar.xz /opt/qt /work/qt
        rm -rf "$WORK/qt"
    fi
    NAME="GauntletDarkLegacy-$VERSION-linux-x86_64"
    D="$OUT/$NAME"
    rm -rf "$D"
    mkdir -p "$D"
    TC=/rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin
    "${RUN[@]}" -v "$PWD/$QT:/opt/qt:ro" -v "$HOME/.rustup:/rustup:ro" -v "${CARGO_HOME:-$HOME/.cargo}:/cargo" -v "$D:/out" \
        -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/work/target -e PATH="$TC:/usr/local/bin:/usr/bin:/bin" \
        "$IMG" bash tools/linux/build-package.sh /out
    payload_common "$D/payload"
    cp release/README.txt LICENSE "$D/"
    library_source linux
    tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
    ;;
windows)
    TC=~/.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin
    # The build machine's folders out of the programs' panic locations (as
    # the Linux container's /cargo and /src); joined with .cargo/config.toml's.
    REMAP="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$PWD=/src"
    RUSTC=$TC/rustc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="$REMAP" \
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
        private/library-src/windows/qtbase-everywhere-src-*.tar.xz private/library-src/windows "$D/LIBRARY_LICENSES.txt"
    cp "$D/LIBRARY_LICENSES.txt" "$D/payload/"
    library_source windows
    (cd "$OUT" && rm -f "$NAME.zip" && 7z a -tzip -bd "$NAME.zip" "$NAME" >/dev/null)
    ;;
*)
    echo "usage: tools/package.sh linux|windows OUT_DIR" >&2
    exit 2
    ;;
esac
source_archive
(cd "$OUT" && sha256sum ./*.tar.gz ./*.zip ./library-source/* 2>/dev/null > SHA256SUMS || true)
ls -la "$OUT"
