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
    from 2020 or later: the GNU C library 2.31 or newer (for example Ubuntu
    20.04, Debian 11, Fedora 32, RHEL 9 or newer). On Linux the game and the
    installer use X11 (Wayland desktops run them through XWayland) and the
    game uses ALSA for sound.
  - A graphics card with Vulkan (Windows: Vulkan or DirectX 12).
  - About 1 GB of disk space (about 3 GB with the optional HD textures).
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

Button prompts: with an Xbox-style controller (or most PC controllers) the
game's on-screen buttons show A, B, X and Y instead of the PlayStation's
cross, circle, square and triangle; with a PlayStation controller, or with
no controller, they stay as the original's. Settings → Button prompts
chooses Automatic (the controller used last), PlayStation or Xbox.


Settings
--------
F10 (or Options → Display on the title, or Settings → Display in the pause
menu) opens the settings: widescreen view, motion smoothing, VSync, frame
cap, fullscreen, anti-aliasing, reduced flashes, the frame-rate counter,
button prompts, "Graphics...", "Original presentation" and player 1's keys.
F11 toggles fullscreen and F3 the frame-rate counter. Options → Screen moves
the picture, as the original's Screen Adjust did for TVs.

"Graphics..." holds the modern picture options. They are not in the original
game and change only how it looks, never how it plays. All are on by default
except HD textures:
    Preset ............... Enhanced (the recommended values) or Original
                           (every option below off); Custom when you have
                           changed one
    Texture filtering .... Sharp: textures stay crisp at your screen's
                           resolution and in the distance
    Sharpening ........... Off, 20 % to 100 % (default 40 %)
    Colour precision ..... Full: smooth gradients without the PS2's 16-bit
                           dither pattern
    Smooth lightmaps ..... Soft edges on the levels' painted-in shadows
    Dynamic lights ....... Up to 32 lights from spells and explosions
                           instead of 8
    Torch light .......... Gentle flickering light from torches and fires
    Character shadows .... Real shadows for players, enemies and items
                           instead of round blobs
    Ambient occlusion .... Soft shading in corners and creases of the levels
                           (anti-aliasing then uses SMAA in place of MSAA)
    Bloom ................ A glow around bright fire and magic
    HD textures .......... Four-times sharper textures (see below)

HD textures are made once, on your computer, from your own disc: turn the
row on and the game generates them in the background (about 2 GB; six and a
half minutes on a Radeon RX 580), showing the progress on the row. They
appear from the next level on; the front end takes them after a restart.
Fire, glows, magic effects and skies keep their original textures. Turning
the row off stops the generator; turning it on again continues where it
stopped.

If the frame rate drops on your computer, turn off Character shadows and
Ambient occlusion first; they cost the most. "Original presentation" on the
main settings page returns the whole picture to the original game's.


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


Network play
------------
Up to four computers can play one game together over a network (not in the
original game; added to this port). Every computer needs this port, the same
version, installed from the same disc.

  - To host: on the title choose Network, then Host Game. The port is 28780
    unless you type another; let it through your firewall (Windows asks the
    first time). Tell the others this computer's address and the port. When
    everyone has joined, press Enter or Start.
  - To join: choose Network, then Join Game, type the host's name or IP
    address (with :port if the host chose another, for example
    192.168.1.20:28780) and press Enter. Typing needs the keyboard.
  - Everyone then sees the select screen, as on the couch: the host's player
    1 is in; everyone else presses Start on their own controller to join.
    Four players in all: with two computers two on each (the keyboard's
    two layouts or controllers), with three two on the host and one on each
    of the others, with four one each.

Everyone shares one game: the characters come from the host's saves and are
saved on the host's computer (the others' own saves are not used), and the
host's game options (difficulty, multiplayer mode) apply. Any player can
pause the game and use the menu, for everyone. Audio, Screen and Display are
each computer's own: change them before the game, or use F10 (the settings
overlay) during it; the game goes on under the overlay.

Every computer runs the whole game and only the players' buttons travel.
Your own presses show at once: each computer draws the game a little ahead,
guessing that the other players keep doing what they did last, and corrects
itself when their buttons arrive. Over the internet another player may
therefore jump back a little when they change direction. If a connection
stops for more than a second, the game waits for it ("Waiting for the other
machines…"). If a player's computer leaves, the game asks for that player's
controller, as when one is unplugged; Start on a free controller takes the
player over. If the host leaves, the game ends for everyone.


Movies
------
The game plays its movies (intro, realm and ending movies) by itself; nothing
has to be installed for them. (Releases up to 1.3.0 needed FFmpeg for the
movies; an ffmpeg.exe left in the install folder is no longer used and can be
deleted.)


Uninstalling
------------
Delete the install folder, and the shortcuts:
    Linux:    ~/.local/share/applications/gauntlet-dark-legacy.desktop and
              the desktop's gauntlet-dark-legacy.desktop
    Windows:  "Gauntlet Dark Legacy" in the Start menu's Programs folder and
              on the desktop
Your saves stay (see Saves) unless you delete them too. Generated HD textures
stay as well, in the hd-textures folder beside saves.


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
the installer are listed with their notices in LIBRARY_LICENSES.txt, and
their source (Qt's; on Windows also the GCC runtime's) is distributed with
this release (library-source). Gauntlet: Dark Legacy and its data belong
to their owners; they are not part of this software.
