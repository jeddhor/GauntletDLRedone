GAUNTLET: DARK LEGACY — native PC port
=======================================

This is a native PC version of the PlayStation 2 game Gauntlet: Dark Legacy
(USA, SLUS-20047). It runs the original game's rules on your computer. It
contains none of the game's own data: the installer reads that from your
own disc (or an image of it) and checks every file against the supported
release. No PlayStation 2 BIOS and no emulator are used.


What you need
-------------
  - Your own copy of the game: the disc in a drive, or an image of it.
  - 64-bit Windows (tested on Windows 11), or a 64-bit Linux distribution
    with the GNU C library 2.43 or newer (for example Ubuntu 26.04). On
    Linux the game uses X11 (Wayland desktops run it through XWayland) and
    ALSA for sound.
  - A graphics card with Vulkan (Windows: Vulkan or DirectX 12).
  - About 1 GB of disk space.
Nothing else has to be installed first: the libraries the installer needs
(Qt) come with it in this package.


Installing
----------
Run GauntletDarkLegacy-Installer and follow its pages:

  1. Put the game disc in a drive (or have a disc image, .iso or .bin, at
     hand). The installer lists drives with a disc; "Disc image file…"
     picks an image and "Folder…" a folder holding the disc's files.
     It says whether the disc is the supported one before you go on.
  2. Choose the install folder (by default GauntletDarkLegacy in your home
     folder) and whether you want a desktop shortcut and a menu entry.
  3. The installer copies the game's programs, then reads, checks and
     extracts the game's files from the disc (about 1 GB). This takes a
     minute or two from an image and longer from a disc.

Nothing is kept of an import that does not finish: run the installer again.
Installing again into the same folder replaces the installation.

Without the installer, the same import can be run from a terminal in the
install folder:
    gdl_import install DISC_OR_IMAGE INSTALL_FOLDER
(INSTALL_FOLDER holding gdl_play, gdl_import and import/ from payload/).


Starting
--------
Use the shortcut, or run gdl_play (Windows: gdl_play.exe) in the install
folder. The game needs a graphics card with Vulkan (Windows: Vulkan or
DirectX 12).


Controls
--------
Player 1, keyboard:
    Move ............. W A S D or the arrow keys
    Attack ........... Space or K          (PS2: cross)
    Magic ............ E or L              (PS2: circle)
    Turbo ............ Shift or I          (PS2: triangle; also closes scrolls)
    Square ........... Q or J
    Dash ............. Z                   (PS2: L1)
    Team move ........ X                   (PS2: L2)
    Strafe ........... Ctrl                (PS2: R1)
    R2 ............... V
    Start / pause .... Enter or Escape
    Select ........... Tab
Player 2, keyboard: T F G H move, R attack, Y magic, U start, B team move.

Controllers: the d-pad and left stick move; the face buttons are the PS2's
(A/cross attack, B/circle magic, Y/triangle turbo, X/square), the shoulder
buttons L1/L2/R1/R2, Start and Select (Xbox: ≡ Menu and ⧉ View). Up to four
players can play with controllers; a controller's Start joins in the Tower.

Menus: arrows or d-pad choose, Enter or A/cross accepts, Escape/Backspace
or Y/triangle goes back. On the "Press Start" screen, Back quits the game.


Settings
--------
F10 (or Options → Display on the title, or Settings → Display in the pause
menu) opens the settings: widescreen view, motion smoothing, VSync, frame
cap, fullscreen, anti-aliasing, reduced flashes, the frame-rate counter,
"Original presentation" and player 1's keys. F11 toggles fullscreen and F3
the frame-rate counter. Options → Screen moves the picture, as the
original's Screen Adjust did for TVs.


Saves
-----
Characters are saved with the game's own Save Character (in the Tower's
menu: Manage Character). They live outside the install folder, so they
survive reinstalling and uninstalling:
    Linux:    ~/.local/share/gdl-native/SLUS-20047/saves
              (or $XDG_DATA_HOME/gdl-native/SLUS-20047/saves)
    Windows:  %APPDATA%\gdl-native\SLUS-20047\saves
settings.json in the same folder holds the settings. Every save keeps a
backup (.bak) of the one before.


Movies
------
The game's movies (intro, realm and ending movies) need FFmpeg. On Linux
install it from your distribution (for example "sudo apt install ffmpeg");
on Windows put ffmpeg.exe into the install folder. Without FFmpeg each movie
is skipped and the game goes on.


Uninstalling
------------
Delete the install folder, and the shortcuts:
    Linux:    ~/.local/share/applications/gauntlet-dark-legacy.desktop and
              the desktop's gauntlet-dark-legacy.desktop
    Windows:  "Gauntlet Dark Legacy" in the Start menu's Programs folder and
              on the desktop
Your saves stay (see Saves) unless you delete them too.


If something goes wrong
-----------------------
If the game cannot start, it writes the reason to
    Linux:    ~/.local/share/gdl-native/last-error.txt
    Windows:  %APPDATA%\gdl-native\last-error.txt
(and on Windows shows it in a message box). KNOWN_ISSUES.txt lists what is
known to differ from the original.


License
-------
This port is free software under the GNU General Public License, version 3
or later (LICENSE). Third-party components and their notices are listed in
THIRD_PARTY.md and THIRD_PARTY_LICENSES.txt; the libraries that come with
the installer are listed with their notices in LIBRARY_LICENSES.txt, and the
source of the Qt they come from is distributed with this release
(qt-source). Gauntlet: Dark Legacy and its data belong to their owners; they
are not part of this software.
