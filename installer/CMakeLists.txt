# Gauntlet: Dark Legacy native port — installer (M21, docs/decisions/0005-installer.md).
# Qt 6 Widgets; the same sources build on Linux and (cross-compiled with
# MinGW-w64) Windows. tools/package.sh builds and packages it.
cmake_minimum_required(VERSION 3.21)
project(gdl_installer VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.2 REQUIRED COMPONENTS Widgets)

qt_add_executable(gdl_installer WIN32
    src/main.cpp
    src/installation.cpp
    src/installation.h
    src/wizard.cpp
    src/wizard.h
)
target_link_libraries(gdl_installer PRIVATE Qt6::Widgets)
target_compile_definitions(gdl_installer PRIVATE GDL_INSTALLER_VERSION="${PROJECT_VERSION}")
set_target_properties(gdl_installer PROPERTIES OUTPUT_NAME "GauntletDarkLegacy-Installer")
if(UNIX AND NOT APPLE)
    # The Linux package carries Qt in lib/ and plugins/ beside the installer
    # (tools/python/bundle_qt_linux.py). DT_RPATH, not RUNPATH: it also
    # applies to the dependencies of those libraries and plugins. Only
    # $ORIGIN/lib: not the build's Qt folder (tools/linux/build-qt.sh).
    set_target_properties(gdl_installer PROPERTIES SKIP_BUILD_RPATH ON)
    target_link_options(gdl_installer PRIVATE "LINKER:-rpath,$ORIGIN/lib" LINKER:--disable-new-dtags)
endif()
if(MINGW)
    # Stripped at link time, without the PE time stamp (strip would insert
    # one), so that rebuilds are byte-identical.
    target_link_options(gdl_installer PRIVATE -static-libgcc -static-libstdc++ -s -Wl,--no-insert-timestamp)
endif()
