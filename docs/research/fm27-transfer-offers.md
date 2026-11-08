# FM27: how incoming transfer offers are stored (macOS, Steam build 26340721)

Read live on 28–29 July 2027 in an Ajax save, from Olympiacos' offer for Sofyan Amrabat (shown in the game as
€22M), with the module's read commands. Nothing here is written by the app yet.

## The offer object

`FULL_TRANSFER_OFFER` (global namespace), 0xD0 bytes, handed out by a pool in game_plugin's `__DATA` (RVA
`0x56B4E28` on arm64: chunks list, first free slot, `u32` free/used, slot size 0xD0, first chunk 7120 slots, later
chunks 128; free slots are chained through their first 8 bytes).

| Offset | Field |
|---|---|
| `+0x00` | vtable |
| `+0x08` | `u32` 1, then a `u32` that is 0 for most offers and 21 for the open offer to the human club |
| `+0x30` | `u32` flags (`0x10000000`, `0x10000040`, `0x40`, ...) |
| `+0x38` | the player's identity block |
| `+0x40`, `+0x48` | the buying team (twice) |
| `+0x50` | the selling team; null for an offer to a free agent |
| `+0x58` | `i32` up-front fee in pounds (Amrabat: 10,342,781) |
| `+0x5C` | `u32` offer id: its index in the transfer manager's list when it was made |
| `+0x60` | `u32` a game-wide sequence number |
| `+0x6C`–`+0x80` | dates (u16 day of year with flags, u16 year): made on, deadline, ...; finished offers have more set |
| `+0x84`, `+0x88` | status bytes |
| `+0xA8` | pointer to the add-ons, or null: (begin, end, cap) lists of 8-byte entries (`u32` amount, `u16` condition, `u16` kind); Amrabat's held 4,180,208 and 4,309,492, which with the fee make the €22M the game shows |

## Where offers are kept

- `TRANSFER_MANAGER+0x08`: one (begin, end, cap) list of every offer of the save (10,401: 7,486 transfers, 594
  loans, 2,310 B-team moves, 11 trials). The only pointer to an offer.
- The game shows an offer to the human club through its inbox item, `TRANSFER_OFFER_NEWS` (about 0x300 bytes, two
  copies, in the game-wide news stream that no list covers). Before it sits a record with the total fee
  (18,832,481), the offer id (10393), the sequence number (20130) and the add-ons. The item points at both clubs,
  their teams, the player, his agent, the human manager, competitions, nations, stadiums and a media source.

## What a test showed

An offer copied from Amrabat's into a free pool slot, for Owen Wijndal from Casciavit Milano (AC Milan's name in
FM27), appended to the manager's list: the game kept it as it was through a day of play and showed it nowhere. An
offer without its inbox item is invisible and inert.

## Open

- Making the inbox item: the news stream has no list to append to; its write position and room are unknown.
- Turning a real incoming offer into another one (buyer, player, fee) means changing the offer, its detail record and
  both copies of the inbox item consistently; what accepting it then does is untested.
