"""Lijst de transferbiedingen in de save, en die voor spelers van de menselijke club.

Posities gemeten op macOS (Steam 26340721), zie docs/research/fm27-transfer-offers.md.
Op Windows eerst controleren: zoek met census("TRANSFER_MANAGER", True) het object, kijk of +0x08 een
(begin, end, cap)-lijst met FULL_TRANSFER_OFFER-pointers is, en of de velden van een bod dezelfde plek hebben
(describe(bod)). Pas de constanten hieronder zo nodig aan.

    python tools/research/offers.py
"""
import struct, sys
from fmq import *

TM_LIST = 0x08          # TRANSFER_MANAGER: (begin, end, cap) van alle biedingen
O_PLAYER = 0x38         # identiteitsblok van de speler
O_BUYER = 0x40          # team van de koper (ook op +0x48)
O_SELLER = 0x50         # team van de verkoper (null: vrije speler)
O_FEE = 0x58            # i32 som vooraf, pond
O_ID = 0x5C             # bodnummer = plek in de lijst
O_SEQ = 0x60            # volgnummer
O_EXTRA = 0xA8          # bonussen
CLUB_NAME = 0xC0
TEAM_CLUB = 0x30


def club_name_of_team(t):
    try:
        c = u64(t + TEAM_CLUB)
        return fmstr(u64(c + CLUB_NAME)) or "?"
    except Exception:
        return "?"


def main():
    tms = [c for c in census("TRANSFER_MANAGER", True) if c["class"].endswith("TRANSFER_MANAGER") and c["instances"]]
    if not tms:
        sys.exit("TRANSFER_MANAGER niet gevonden (is er een save geladen?)")
    tm = int(tms[0]["first"][0], 16)
    offers = plist(tm + TM_LIST)
    print("TRANSFER_MANAGER", hex(tm), "|", len(offers), "biedingen")
    team = chain()["team"]
    for idx, o in enumerate(offers):
        if class_of(o) != "FULL_TRANSFER_OFFER":
            continue
        b = rd(o, 0xD0)
        w = lambda off: struct.unpack_from("<Q", b, off)[0]
        i = lambda off: struct.unpack_from("<i", b, off)[0]
        if w(O_SELLER) != team:
            continue
        print("index %d  %s  koper %s  som £%d  nr %d  volg %d  bonussen %s" % (
            idx, hex(o), club_name_of_team(w(O_BUYER)), i(O_FEE), i(O_ID), i(O_SEQ), hex(w(O_EXTRA))))


if __name__ == "__main__":
    main()
