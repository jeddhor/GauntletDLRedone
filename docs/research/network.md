# Network play (NATIVE, 2026-11-02; prediction and time sync for release 1.3.0)

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
  itself included, and every machine applies the frames in order. Release
  1.2.0 sent a machine's input for step N + delay when it applied step N
  (delay 4, 133 ms), so every late frame held everyone and a round trip above
  the delay slowed the game; from release 1.3.0 (protocol 2, the user's
  request for internet play, 2026-11-02) the parts below take over.
- Local clock (net_local_tick): each machine samples its ports and menu
  events at 30 Hz of real time and sends them as its input for the next step
  (`input_next`), whether or not the other machines' inputs have come; the
  first `delay` inputs (nothing pressed; delay 2, 67 ms) go out at the start.
  The clock waits ("Waiting for the other machines…" where the frame rate is
  shown) only when it is 30 steps (one second) ahead of the confirmed steps.
- Time sync: the host notes when each machine's input for a step arrives and
  reports every 10 frames how long before the step's last input each
  machine's came (Sync, a moving average). A machine more than 50 ms (1.5
  steps) early samples at 27 Hz until it is not, so the machines' inputs meet
  at the host and none waits on another's clock. Clients ping the host every
  0.5 s (Ping / Pong; RFC 6298's smoothed round trip, reported in NETSTATS).
- Confirmed steps: simulate applies a frame when it is there; the network
  game's fixed clock runs at 60 Hz, so a machine that fell behind catches up.
- Prediction (presentation only): the confirmed game is run ahead to this
  machine's latest input — its own pads as sampled, the other machines'
  assumed to keep their last confirmed pads — and that copy is drawn
  (net_predict, after the front end each frame, swaps it into Sim::game;
  net_frame_start swaps the confirmed game back before the next step). A
  player sees their own presses at once; other players move on their last
  input until theirs arrives. When a confirmed step differs from what the
  copy assumed (another machine's pads, menu events, a step off the plain
  play path, or the state's digest), the copy is rebuilt from the confirmed
  game; it extends at most 3 steps a frame (C2: about 2 ms a step, 1.9 ms a
  copy, 0.24 MB of records). It runs only in plain play (no screen, box,
  menu, pause, level change or saver) and stops at a step that leaves it
  (predict_step: CheckSoftReset, the saver's check, the Tower's join check,
  the step, clear_player — simulate's plain path). Its sound effects play at
  once; the confirmed step does not play them again (without_played).
  Everything else — menus, transitions, saves, music, hashes — follows the
  confirmed game. GDL_NET_PREDICT=0 (TEST) draws the confirmed game.
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
  same two steps on every machine. More steps run in one frame only while
  each leaves the game quiet (net_quiet: plain play, no reset request,
  someone in play, not the every-player-dead shop, no menu events in the
  frame) — then the front end would act on none of them, whenever it runs —
  so a machine rendering below 30 Hz keeps the game's pace (net-check-p3:
  15 fps per window, the play no longer waits). The shared step counter
  advances only on applied steps; the waits that depend on each machine's own
  timing (the level-change banner and loading) apply none.
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

## Protocol (version 2)

Messages are a u32 little-endian length and a type byte: Hello ("GDLNET",
protocol, version string, data id), Welcome, Refuse (reason), Lobby (machine
count), Start (machines, your number, delay, GamePrefs, the eight card
files), Input (step, pads, events), Frame (step, eight pads, every machine's
events, host flags, gone machines), Hash (step, SHA-256), End (reason), and
from version 2 Ping / Pong (id) and Sync (per machine, ms early). Version 1
(release 1.2.0) is refused. A pad
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
- The prediction is drawn only: another player's change of direction shows
  late by the round trip and their character may jump back a little when it
  arrives (worse on long routes); a predicted sound can play for an action
  that the confirmed step does not have. No rollback of the game itself.
- IPv6 hosting (the host binds 0.0.0.0); joining by an IPv6 address works in
  brackets.
- Windows: the firewall asks to allow the host's port on the first host
  (the LAN check added a rule instead).

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
- Release 1.3.0 (local clock, time sync, prediction, quiet steps; protocol
  2), the same scenario (G1, 900 updates, a shared pause) on the private
  Xvfb in 640×448 windows (GDL_WINDOW; two software renderers reach about
  15 fps each, not 30), GDL_NET_LAG_MS / GDL_NET_JITTER_MS simulating a
  distant network on every message each machine sends:
  - Costs measured in the client (GDL_NET_BENCH scratch, 2026-11-02, two
    players at update 300): a copy of the game 1.6 / 1.9 / 1.4 ms and a step
    0.63 / 2.19 / 0.50 ms in G1 / C2 / J5; the hashed records 0.22 / 0.24 /
    0.08 MB.
  - net-check-p2 (no lag, one step per frame): equal, but at 15 fps the
    confirmed steps came at 15 a second and the clocks waited 41–46 of 87 s;
    net-check-p3 with quiet steps: 59 s, the waits (9–13 s) all outside play.
  - net-check-p4-lag was invalid: the simulator delayed each message from
    when the writer took it, so the queue grew (a 2.8 s round trip); fixed
    to delay from when it was queued.
  - net-check-p5-lag, 60 ms one way + up to 30 ms (round trip measured
    229 ms, frame granularity included): during play the host sampled 888
    steps in 30.4 s (29.2 a second) and waited 0.17 s; the joining machine
    856 in 29.3 s (29.3) and waited 0.41 s; its prediction ran 5.8 steps
    ahead on average (at most 11). All 900 hashes equal, the prediction's
    digest equal to the confirmed state's at every checked step (0
    mismatches).
  - net-check-p6-lag100, 100 ms + up to 50 ms (round trip 315 ms): 29.0 and
    29.4 steps a second during play, 0.34 s of waiting on each machine,
    prediction 7.9 steps ahead on average (at most 14); all equal, 0
    mismatches.
  - net-check-p7-wine-lag, as p5 with the Windows build joining under Wine:
    all 900 equal, and the Windows build's 900 hashes byte-identical to
    net-check-p5-lag's Linux host; the host waited 1.0 s in 31.9 s of play
    (Wine's client renders slower).
- Two computers on a LAN (2026-11-13, the owner approving the use of their
  Windows 11 desktop, GTX 1080, 10.1.0.91; this Linux machine on the
  private Xvfb with lavapipe; the same source, d05c906 + the wgpu-hal
  patch, its Windows build with internal_error_panic; scratch harnesses
  net_lan.py / net_lan_win.py with the net_check scripts; the desktop's
  machine run by a scheduled task in its session with a test APPDATA,
  empty saves, the original presentation, no audio, gamepads or attract):
  - net-lan-lan1 (Linux hosts): equal at update 300, then both machines
    waited in the East Wing door's box ("You need 20 Golden Eagle
    Feathers"), which the joining machine's player had opened and only that
    player's triangle closes (ScrollMessageBox's mask); the script pressed
    triangle on the host only. The same happened on loopback
    (net-check-loop132); net_check's scripts now press triangle on both
    machines every 25 steps, and the host's extra updates are 10 (45 left
    loop132c in the departed player's controller box). net-check-loop132d:
    passed, 900/900.
  - net-lan-lan2 (Linux hosts, the desktop joins): all 900 hashes equal,
    the dumps at updates 300 and 880 equal, ports 0 and 1, both ended
    normally; the host's 900 hashes byte-identical to loop132d's, the
    desktop's last equal to loop132d's last (962f5e54…).
  - net-lan-lan3 (the desktop hosts, Linux joins; an inbound Windows
    Firewall rule for the test program on the Private profile added over
    SSH and removed afterwards): all 900 equal, both dumps equal, both
    ended normally, the last hash again 962f5e54….
- Not covered: the internet (latency, loss, NAT), a Windows-to-Windows
  game, the firewall prompt itself (the rule was added), the shop, map and
  movie paths over two machines.
