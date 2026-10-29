#!/bin/bash
# Release packages (M21, docs/decisions/0005-installer.md).
#
#   bash tools/package.sh linux   OUT_DIR
#   QT_WIN_DIR=/path/to/Qt/6.x.y/mingw_64 bash tools/package.sh windows OUT_DIR
#
# Each package is a folder (and an archive of it) holding the Qt installer
# and payload/: gdl_play, gdl_import, import/ (catalog.json,
# supported-disc.json), LICENSE, THIRD_PARTY.md, THIRD_PARTY_LICENSES.txt,
# README.txt, KNOWN_ISSUES.txt. Game data is never packaged: the installer
# imports it from the user's disc. A source archive of HEAD goes beside the
# packages (the GPL's corresponding source).
#
# Linux: the installer links the system's Qt 6. Windows: the game and the
# importer are cross-built with the pinned Rust toolchain's
# x86_64-pc-windows-gnu target (docs/toolchain.md); the installer with
# MinGW-w64 against a Qt for Windows (MinGW, the same version as the host's
# Qt tools) given as QT_WIN_DIR, whose DLLs and platform plugin are copied.
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
    payload_common "$D/payload"
    cp release/README.txt LICENSE "$D/"
    tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
    ;;
windows)
    : "${QT_WIN_DIR:?set QT_WIN_DIR to a Qt for Windows (MinGW) directory, e.g. .../Qt/6.10.2/mingw_64}"
    TC=~/.rustup/toolchains/1.93.1-x86_64-unknown-linux-gnu/bin
    RUSTC=$TC/rustc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        CARGO_TARGET_DIR=target-win "$TC/cargo" build --release --offline --locked \
        --target x86_64-pc-windows-gnu -p gdl_client -p gdl_install --bin gdl_play --bin gdl_import
    cmake -S installer -B private/build/installer-windows -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_SYSTEM_NAME=Windows -DCMAKE_C_COMPILER=x86_64-w64-mingw32-gcc-posix \
        -DCMAKE_CXX_COMPILER=x86_64-w64-mingw32-g++-posix -DCMAKE_RC_COMPILER=x86_64-w64-mingw32-windres \
        -DCMAKE_PREFIX_PATH="$QT_WIN_DIR" -DQT_HOST_PATH=/usr -DCMAKE_FIND_ROOT_PATH="$QT_WIN_DIR" >/dev/null
    cmake --build private/build/installer-windows
    NAME="GauntletDarkLegacy-$VERSION-windows-x86_64"
    D="$OUT/$NAME"
    rm -rf "$D"
    mkdir -p "$D/payload" "$D/platforms" "$D/styles"
    cp private/build/installer-windows/GauntletDarkLegacy-Installer.exe "$D/"
    for dll in Qt6Core Qt6Gui Qt6Widgets; do cp "$QT_WIN_DIR/bin/$dll.dll" "$D/"; done
    for rt in libgcc_s_seh-1 libstdc++-6 libwinpthread-1; do
        [ -f "$QT_WIN_DIR/bin/$rt.dll" ] && cp "$QT_WIN_DIR/bin/$rt.dll" "$D/"
    done
    cp "$QT_WIN_DIR/plugins/platforms/qwindows.dll" "$D/platforms/"
    cp "$QT_WIN_DIR"/plugins/styles/*.dll "$D/styles/" 2>/dev/null || true
    cp target-win/x86_64-pc-windows-gnu/release/gdl_play.exe target-win/x86_64-pc-windows-gnu/release/gdl_import.exe "$D/payload/"
    x86_64-w64-mingw32-strip "$D/payload/gdl_play.exe" "$D/payload/gdl_import.exe"
    payload_common "$D/payload"
    cp release/README.txt LICENSE "$D/"
    (cd "$OUT" && rm -f "$NAME.zip" && 7z a -tzip -bd "$NAME.zip" "$NAME" >/dev/null)
    ;;
*)
    echo "usage: tools/package.sh linux|windows OUT_DIR" >&2
    exit 2
    ;;
esac
source_archive
(cd "$OUT" && sha256sum ./*.tar.gz ./*.zip 2>/dev/null > SHA256SUMS || true)
ls -la "$OUT"
