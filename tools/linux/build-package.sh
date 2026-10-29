#!/bin/bash
# The Linux package's programs (M21; docs/toolchain.md "Linux release
# build"). Runs inside gdl-build-focal (tools/package.sh starts it):
#
#   build-package.sh OUT_DIR
#
# with the repository at the working folder (read-only), the pinned Rust
# toolchain on PATH, CARGO_HOME (offline registry) and CARGO_TARGET_DIR set,
# Qt (tools/linux/build-qt.sh) at /opt/qt and /work writable. OUT_DIR gets
# the installer with its Qt (bundle_qt_linux.py), LIBRARY_LICENSES.txt and
# payload/{gdl_play,gdl_import}; tools/package.sh adds the rest. Every ELF
# file of the package is checked against the floor: the GNU C library 2.31
# (Ubuntu 20.04) and GCC 10's libstdc++ (GLIBCXX_3.4.28).
set -euo pipefail
OUT=${1:?usage: build-package.sh OUT_DIR}
cargo build --release --offline --locked -p gdl_client -p gdl_install --bin gdl_play --bin gdl_import
cmake -S installer -B /work/installer -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH=/opt/qt \
    -DCMAKE_CXX_COMPILER=g++-10 >/dev/null
cmake --build /work/installer
mkdir -p "$OUT/payload"
cp /work/installer/GauntletDarkLegacy-Installer "$OUT/"
cp "$CARGO_TARGET_DIR/release/gdl_play" "$CARGO_TARGET_DIR/release/gdl_import" "$OUT/payload/"
strip "$OUT/payload/gdl_play" "$OUT/payload/gdl_import" "$OUT/GauntletDarkLegacy-Installer"
python3 tools/python/bundle_qt_linux.py "$OUT" /opt/qt
python3 tools/python/library_licenses.py linux "$OUT" /opt/qt/sbom/qtbase-*.spdx \
    private/library-src/linux/qt6-base_*.orig.tar.xz private/library-src/linux "$OUT/LIBRARY_LICENSES.txt"
newest() { sort -uV | tail -1; }
glibc=$(find "$OUT" -type f -exec sh -c 'file -b "$1" | grep -q ^ELF && objdump -T "$1"' _ {} \; | grep -o 'GLIBC_[0-9.]*' | newest)
glibcxx=$(find "$OUT" -type f -exec sh -c 'file -b "$1" | grep -q ^ELF && objdump -T "$1"' _ {} \; | grep -o 'GLIBCXX_[0-9.]*' | newest)
echo "newest symbol versions needed: $glibc $glibcxx"
[ "$(printf '%s\n' "$glibc" GLIBC_2.31 | newest)" = GLIBC_2.31 ] || { echo "above the floor: $glibc" >&2; exit 1; }
[ "$(printf '%s\n' "$glibcxx" GLIBCXX_3.4.28 | newest)" = GLIBCXX_3.4.28 ] || { echo "above the floor: $glibcxx" >&2; exit 1; }
