# Bot route hints (DEBUG traversal tooling)

One file per level (`<W><L>.json`, bot.rs `Hint` list), made by
`tools/python/route_hints.py` from a traversal-bot run that ended the level
alive: the triggers and treasures it reached, in order, then waypoints of its
way to the exit. `game_run.py` passes the file to every `--bot` and
`--explore` run of that level (`GDL_BOT_HINTS=0` runs without,
`GDL_BOT_HINTS=FILE` with another). The bot still drives port 0 through the
ordinary pad path; the hints only choose its goals. Not game data.

| Level | Source run |
|---|---|
| G2 | scratch g2-f34 (assisted bot, regular exit at update 33,095) |
| E1 | campaign-t21-E1-bot (assisted, 63,780, the soul taken) |
| F1 | campaign-t21-F1-bot (assisted, 115,534) |
| I3 | private/reports/bot-b48-I3.json (assisted bot f87, the rune taken, exit at update 148,473) |
| D4 | private/reports/bot-b53-D4.json (assisted bot f95, the sword taken, exit at update 36,377) |
| H3 | campaign-t26-H3-bot3 (assisted chain bot f98, the rune taken, 143,648) |
| I2 | private/reports/bot-b37-I2.json (assisted bot, the rune taken) |
| J1 | private/reports/bot-b37-J1.json (assisted bot) |
| J2 | private/reports/bot-b37-J2.json (assisted bot) |
| J4 | private/reports/bot-b37-J4.json (assisted bot) |
| J6 | private/reports/bot-b62-J6.json (assisted bot c38 with ride edges, the rune taken, exit at update 146,631) |
| C3 | private/reports/bot-b64b-C3.json (assisted bot c39, start delay 149, the bellows taken, exit at update 128,846) |
| I4 | private/reports/bot-b68a-I4.json (assisted bot c47: trigger 534 struck through wall 2644, the lamp taken, exit at update 131,440) |
| J3 | private/reports/bot-b70-263-J3.json (assisted bot c49, start delay 263: trigger 502 struck through wall 1512, exit at update 123,052) |
| D3 | private/reports/bot-b73b-D3.json (assisted bot c52 with run-ups at step-ups, start delay 149: up the bridge pieces raised by pads 50–47, regular exit at update 43,203; b73a left by the secret exit at 32,207) |
