#!/bin/bash
# Qt for the Linux installer (M21; docs/toolchain.md "Linux release build").
# Runs inside gdl-build-focal (tools/linux/Containerfile):
#
#   build-qt.sh QT_SOURCE_TAR PREFIX WORK_DIR
#
# QT_SOURCE_TAR is the qtbase source shipped beside the packages
# (private/qt-src/ubuntu/qt6-base_6.10.2+dfsg.orig.tar.xz: Debian's repack
# of qtbase-everywhere-src-6.10.2, unmodified here), built with GCC 10 on
# Ubuntu 20.04 so that it runs with the GNU C library 2.31 and libstdc++ of
# GCC 10. Only what the installer uses: Core, Gui, Widgets, DBus (the IBus
# input method) and the X11 (XCB) platform plugin — no OpenGL, Vulkan,
# Wayland (Wayland desktops run it through XWayland, as they run the game),
# network, SQL, printing or XML; HarfBuzz, PCRE2, libpng,
# double-conversion and the other third-party code built in from Qt's own
# copies; FreeType and Fontconfig the system's (Fontconfig needs the
# system's FreeType; every desktop has both). The install holds Qt's SBOM
# (sbom/), read by library_licenses.py.
set -euo pipefail
SRC_TAR=${1:?usage: build-qt.sh QT_SOURCE_TAR PREFIX WORK_DIR}
PREFIX=${2:?usage: build-qt.sh QT_SOURCE_TAR PREFIX WORK_DIR}
WORK=${3:?usage: build-qt.sh QT_SOURCE_TAR PREFIX WORK_DIR}
mkdir -p "$WORK"
tar -C "$WORK" -xJf "$SRC_TAR"
SRC=$(echo "$WORK"/qtbase-everywhere-src-*)
cmake -S "$SRC" -B "$WORK/build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_C_COMPILER=gcc-10 -DCMAKE_CXX_COMPILER=g++-10 \
    -DBUILD_SHARED_LIBS=ON -DQT_BUILD_EXAMPLES=OFF -DQT_BUILD_TESTS=OFF -DQT_BUILD_BENCHMARKS=OFF \
    -DQT_GENERATE_SBOM=ON \
    -DINPUT_opengl=no -DFEATURE_vulkan=OFF -DFEATURE_wayland=OFF \
    -DFEATURE_eglfs=OFF -DFEATURE_linuxfb=OFF -DFEATURE_vnc=OFF -DFEATURE_libinput=OFF -DFEATURE_evdev=OFF \
    -DFEATURE_tslib=OFF -DFEATURE_mtdev=OFF \
    -DFEATURE_network=OFF -DFEATURE_sql=OFF -DFEATURE_printsupport=OFF -DFEATURE_concurrent=OFF \
    -DFEATURE_xml=OFF -DFEATURE_testlib=OFF -DFEATURE_openssl=OFF \
    -DFEATURE_icu=OFF -DFEATURE_glib=OFF -DFEATURE_libudev=OFF -DFEATURE_journald=OFF -DFEATURE_gtk3=OFF -DFEATURE_cups=OFF \
    -DFEATURE_system_freetype=ON -DFEATURE_system_harfbuzz=OFF -DFEATURE_system_pcre2=OFF \
    -DFEATURE_system_png=OFF -DFEATURE_system_jpeg=OFF -DFEATURE_system_doubleconversion=OFF \
    -DFEATURE_system_libb2=OFF -DFEATURE_system_textmarkdownreader=OFF -DFEATURE_system_zlib=ON \
    -DFEATURE_xcb=ON -DFEATURE_xkbcommon=ON -DFEATURE_fontconfig=ON -DFEATURE_dbus=ON
cmake --build "$WORK/build" --parallel
cmake --install "$WORK/build"
