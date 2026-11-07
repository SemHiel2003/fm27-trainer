# Technical notes

Everything here was measured on **FM27, Steam build 26340721, Apple silicon** (game_plugin LC_UUID
`F4540355-2186-39CB-9119-C706A6A35346`) and checked against a real save and the running game.

## Architecture

```
FM27 Trainer.app ──(Unix socket: one JSON line per request)──▶ libfm27_agent.dylib inside FM27
   │  egui UI, OCR, planner                                      │  RTTI lookup, mach_vm_read/write
   └─ starts FM27 with DYLD_INSERT_LIBRARIES ────────────────────┘  writes only after a check + undo log
```

- FM27 on macOS is signed with `com.apple.security.cs.allow-dyld-environment-variables` and
  `disable-library-validation`, so a library can be loaded at launch with SIP enabled. An external reader would
  need `task_for_pid`, which the game does not allow.
- The app launches `fm.app/Contents/MacOS/Football Manager 27` directly with `SteamAppId=4530370`, so Steam does
  not relaunch it without the library.
- The agent finds classes by Itanium RTTI: an object starts with its vtable pointer, `[vtable-8]` is the
  type_info, `[type_info+8]` the mangled name (`N2db16HUMAN_NON_PLAYERE`).
- Writes go through `mach_vm_write` and only happen when the bytes still equal what the agent read. Every write
  is logged for undo. On an unknown `game_plugin` UUID the agent refuses to write.

## Memory layout (macOS)

| Chain | Offsets |
|---|---|
| Human manager | class `db::HUMAN_NON_PLAYER`; person block shifted `0x340`; contract `+0x4B0`; contract owner `+0x08` = manager `+0x548` |
| Contract → team → club | team `contract+0x10`, club `team+0x30`, club UID `club+0x0C`, name `club+0xC0` |
| Finances | `club+0x150` → balance `+0x0C`, transfer budget `+0x7C4`, total wage `+0x808`, wage budget `+0x814` (i32, pounds) |
| Squad | `team+0x38` = list (begin, end, cap) of player objects |
| Player | morale `+0x264` (u8, 1–20), condition `+0x254`, sharpness `+0x250`, injury record `+0xF0`, contract `+0x2F0`, UID `+0x394`, names `+0x298/+0x2A0/+0x2A8` (player-coaches shift person fields by `+0xF8`) |
| Injuries | injury record starts with a list of current injuries; the game heals by setting `end = begin`; absences (bans have `0xFFFF` at `+0x16`) at record `+0x18` |
| Happiness | contract `+0x5A` (i16), reasons-to-be-unhappy pointer at contract `+0x30` |
| In-game date | `team+0xA0` → `+0x94`: u16 day-of-year (low 9 bits) + u16 year |
| Training calendar | a per-team block in the training manager holds the team pointer; `+0x28` is a list of week records (`0x60` bytes): `+0x0C` Monday's date, `+0x10` 7 day blocks of `[u16 0, session1, session2, extra]` |

Player, contract, club and finance offsets match the public layout files of FMLibreEditor and FMSuperScout;
the training calendar and in-game date path were measured for this project.

## Session codes

| Range | Category | Codes |
|---|---|---|
| 0–7 | Tactical Understanding | Passing Style, Build Up, Chance Creation, In Possession Shape, Line of Engagement, Pressing Intensity, Pressing Trap, Out Of Possession Shape |
| 100–108 | Match Preparation | Press Avoidance, Ball Progression, Final Third, Defensive Block, Defensive Positioning, Defensive Engagement, Corners, Free Kicks, Penalties |
| 200–228 | Footballing Development | Individual Development, Attacking/Defensive/Possession Ball Work, … |
| 300–307 | Physical & Recovery | Rest, Recovery, Light Conditioning, Conditioning, Heavy Conditioning, … |
| 400–402 | Extra-Curricular | Team Bonding, Community Outreach, Travel |
| 500 | Match | Match |

The order follows `training/training_sessions` in FM27's game data and was confirmed against the calendar
in a save (for example day `[200, 3, 7]` = Individual Development / In Possession Shape / Out Of Possession Shape).

## FM data files

- `.fmf` is an SI archive. `02 01 "afe."` files hold records of `u32 16, u32 16, key[16], IV[16]`, then
  AES-128-CTR over a zstd frame (the key is in the file). `02 01 "fmf."` files are the same without encryption,
  and FM accepts both. A zstd manifest after the footer lists name, extension, offset (from byte 26), stored
  size, raw size and two timestamps. See `tools/fmf_unpack.py`.
- `game_plugin.bundle/…/game_simulation/training.fmf` contains `training_sessions.jsb` (59 sessions),
  `training_session_categories.jsb` and `training_schedule_templates.jsb` (99 presets).
- `.jsb` is SI's binary JSON: the low nibble of each type byte is the type (`a` object, `9` array, `8` string,
  `2` int, `1` bool), and the high nibble is an inline count/length/value plus one, or 0 when a u32 follows. Inline
  ints are `n - 8`. See `tools/jsb.py`.

## Screen reading

The Breakdown pies fill clockwise from 12 o'clock. Each pie is measured in 36 angular sectors, so the dark
spokes between segments don't count. The score blends the filled share with the hue (red 0 → green 120°). The
component bars are read from the coloured marker between the grey ticks.

## Testing hooks

`FM27_PAGE=training|planning|club|squad|settings`, `FM27_LANG=nl|en`, and `FM27_SNAPSHOT=out.png` (the app
saves a screenshot of its own window and quits, which works even when FM is full screen). The CLI has
`fm27-trainer analyze <png> [--save]`, `train [--write]`, `agent '<json>'` and `start-fm`.
