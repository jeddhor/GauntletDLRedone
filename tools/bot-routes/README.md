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
