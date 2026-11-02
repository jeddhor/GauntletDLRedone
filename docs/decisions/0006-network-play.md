# 0006: Network play — lockstep over TCP

The master plan (§5.2) leaves online multiplayer and rollback networking out
of the initial delivery. After release 1.1.0 the project owner asked for it
(2026-11-02): a Network entry on the title menu between Start and Options,
hosting or joining a game by address and port, then the couch co-op select
screen as normal.

Decision:

- **Lockstep, not state streaming.** Every machine runs the whole game and
  only inputs travel; the simulation is deterministic across the Linux and
  Windows builds (equal per-update state hashes) and independent of
  presentation, so equal inputs give equal games. A state hash compared
  every second ends a game that diverges instead of letting it drift.
- **TCP through the host** (std::net, no new dependencies): the host merges
  every machine's input for a step and sends the merged frame to all. Input
  delay 4 steps (133 ms); no prediction or rollback (a slow machine or
  connection makes everyone wait).
- **The couch model.** Machines share the game's controller ports 0..3, so
  remote players join on the select screen with Start as extra controllers
  do; the menus are shared (anyone pauses everyone); the host's memory card
  and game options are the game's. Each machine's own settings (audio,
  screen position, graphics) stay its own and are disabled in the shared
  menus.
- **Native and labelled.** Everything here is an addition (research/
  network.md); no fidelity claim rests on it, and single play is unchanged
  (the same code paths when no network game runs).

Not done: joining a game in progress, IPv6 hosting, NAT traversal or a
lobby server (players connect by address), prediction or rollback.
