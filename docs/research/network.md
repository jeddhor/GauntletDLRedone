# Network play (NATIVE, 2026-11-02)

The original has no network play; the master plan (§5.2) left online
multiplayer out of the initial delivery. The user asked for it after release
1.1.0: a Network entry on the title menu between Start and Options, Host or
Join by address and port, then the couch co-op select screen as normal. Every
part of this is native; none of it is a fidelity claim.

## Design: deterministic lockstep

Every machine runs the whole game; only inputs travel. The simulation is
already deterministic: the per-update state hashes of the Linux and Windows
builds are equal (release.md "Release 1.1.0": the Windows build under Wine,
600/600), and presentation settings do not change them (enhancements.md).

- Transport: TCP, a star through the host (gdl_client::net; std::net, a
  reader and a writer thread per connection, so the game never blocks on a
  socket). No new crates.
- Steps: for each 30 Hz step every machine sends the host its local
  controllers' pad states and its menu events (an [`Input`]); the host merges
  every machine's input for that step into one Frame and sends it to all,
  itself included. A machine sends its input for step N + delay when it
  applies step N; the first `delay` inputs (nothing pressed) go out at the
  start. Default delay 4 steps (133 ms); no machine runs more than that ahead
  of the slowest, and all wait when a frame is late ("Waiting for the other
  machines…" where the frame rate is shown).
- Ports: the game reads controller ports 0..3. ReadControls 0x104b60 makes
  Start edges only on the ports of players 0..3 (an unassigned player k reads
  port k), so a player on ports 4..7 could never join (the first two-machine
  run, net-check-v1, used 4..7 for the joining machine: its Start did
  nothing). Ports 0..3 are dealt in turn (net::ports_of): two machines 0 and 2
  / 1 and 3, three 0 and 3 / 1 / 2, four one each. Each machine's first player
  is player m + 1; its local players are its keyboard (two layouts) and pads
  as in single play.
- Joining: the host's player 1 starts on the select screen as after the
  title's Start (init_player_select(0), player 1 on port 0); the other
  players press Start on their own ports (check_active_players), as a second
  controller does on the couch.
- One step per rendered frame. front_system and advance_level run after the
  fixed steps each frame and decide things from the game state (the pause
  menu, the game-over check, level, shop, map and select transitions,
  character models). With at most one step per frame they act between the
  same two steps on every machine. The shared step counter advances only on
  applied steps; the waits that depend on each machine's own timing (the
  level-change banner and loading) apply none.
- Menus are shared, as on the couch: every machine's menu keys and pause
  toggle travel in its input, and every machine applies all of them after the
  step that carries them. The menus' timed parts move by steps (two vblanks
  each) instead of the frame time: the burn ending (serve_menu_ending) and
  the inventory's closing swirl. The options lists' Audio, Screen and Display
  entries are each machine's own settings and are disabled in a network game
  (greyed, the cursor skips them); the settings overlay (F10) stays local and
  the game goes on under it.
- The host decides what each machine's own playback would: the end of a
  music stream part (FLAG_STREAM_DONE) and of a game movie
  (FLAG_MOVIE_DONE), each raised once and carried in one frame. A client's own
  movie may end earlier or later; the game follows the host's.
- Saves: the host's memory card. Its eight files go to every machine at the
  start; the select screen loads and writes that copy on every machine
  (NetCard), and the host also writes its own disk. Joining machines' saves
  are neither read nor written. The host's GamePrefs (difficulty,
  multiplayer mode, compass) apply; each machine's own come back when the
  game ends, and choices in a network game are not stored.
- Checks: every 30 steps each machine reports a SHA-256 of the state before
  that step (the records sim_state_hash covers); the host compares them, and
  a difference ends the game on every machine ("desynchronised").
- Leaving: a client that leaves (or is silent for 15 s while the others wait)
  is marked gone in the next frame; its ports read as unplugged, and the game
  asks for its players' controllers as it does on the couch
  (ControllerMessageBox) — Start on a free port takes a player over. When the
  host leaves, the clients' games end with a message on the title.
- Refused: another version or protocol, other game data (the executable's
  SHA-256 differs), a full game (four machines), or a game already started.

## Protocol (version 1)

Messages are a u32 little-endian length and a type byte: Hello ("GDLNET",
protocol, version string, data id), Welcome, Refuse (reason), Lobby (machine
count), Start (machines, your number, delay, GamePrefs, the eight card
files), Input (step, pads, events), Frame (step, eight pads, every machine's
events, host flags, gone machines), Hash (step, SHA-256), End (reason). A pad
is a flags byte (connected, analog, stick) and the 22 two-byte entries of
PadStatus. Menu events: 1..9 Up, Down, Left, Right, Accept, Back, Center,
Start, Erase; 0x10 the pause toggle; 0x80 | letter a typed letter (the
select screen's names, to the machine's first port's player). The host
listens on TCP 28780 (NATIVE default) on IPv4.

## Not covered

- Late joining: machines join in the lobby; a game in progress refuses new
  ones.
- The Tower menu's Shop, Inventory and Manage Character act for player 1, as
  in single play in this port.
- No rollback or prediction: a slow connection makes every machine wait.
- IPv6 hosting (the host binds 0.0.0.0); joining by an IPv6 address works in
  brackets.
- Windows: the firewall asks to allow the host's port on the first host.

## Verification

- Synthetic (gdl_client::net tests): every message round-trips; a host and
  two clients on localhost — the lobby, a version refusal, the start and its
  card, merged frames, a host flag in one frame only, a client leaving (its
  ports unplugged), a hash difference ending the game on both.
- Menu tests: the title's three entries, the Network, Host, Join and lobby
  pages, the disabled entries in a network game (and an options list opened
  on one starting at the next), hosting again on a port just left.
- Actual disc, two copies of gdl_play on this machine over 127.0.0.1, on the
  private Xvfb with lavapipe (tools/python/net_check.py; each with its own
  HOME and empty saves, no audio, no gamepads, the original presentation,
  pads from GDL_NET_SCRIPT by network step):
  - net-check-v1: the joining machine on ports 4..7 — its Start never
    joined (ReadControls, above); both games equal to update 300 with one
    player. Fixed by dealing ports 0..3.
  - net-check-v2: the Tower, 450 updates: every per-update state hash equal
    (all 450 distinct: the game advanced), JUSTIN (host, port 0) and SPENCR
    (joining machine, port 1) both in.
  - net-check-v3 (stopped): walking into a realm door opened its "You need
    100 Red Crystals" box, which waits for triangle — the game's own modal
    wait, on both machines; the script then pressed triangle throughout.
  - net-check-v5: --level G1 (GDL_DEBUG_LEVEL: G1 after the select screen),
    900 updates, both walking and the host attacking; the joining machine
    paused everyone, moved in the shared menu, opened Settings, backed out
    and resumed (menu events in GDL_NET_SCRIPT). All 900 hashes equal, the
    dumps at updates 300 and 880 equal (positions (24.38, 0, 2.5) → (7.23,
    0, −3.58) and (26.20, 0, 6.06) → (32.29, 0, 3.18)), the same pause
    sequence in both UI traces.
  - net-check-v6-wine: as v5 with the Windows build (x86_64-pc-windows-gnu)
    joining under Wine 10.0 (WGPU_BACKEND=vulkan to lavapipe — Wine's
    Direct3D 12 failed wgpu's compute pipelines, v6-wine-dx12; in a Wine
    virtual desktop — on the bare Xvfb winit's monitor modes assert,
    v6-wine-monitor): 900/900 equal, and the Windows build's 900 hashes are
    byte-identical to net-check-v5's Linux host.
- Unit and menu tests do not reach a real network: every run above is one
  machine over loopback.
