"""Onderzoekshulp voor FM27 (alleen lezen, tenzij je zelf `write` gebruikt).

Praat met de module van FM27 Trainer:
- macOS: via de socket van de module die met FM meedraait (~/Library/Application Support/FM27 Trainer/agent.sock);
- Windows: via `fm27-trainer.exe agent-serve` (de app leest fm.exe zelf; één proces, veel opdrachten).

Gebruik:
    python -i tools/research/fmq.py
    >>> q({"cmd": "ping"})
    >>> chain()                      # manager -> contract -> team -> club -> financiën
    >>> census("TRANSFER", True)     # klassen met die naam + aantal objecten (RTTI)
    >>> describe(0x...)              # een object als woorden met wat ze lijken
    >>> refs(0x...)                  # wie wijst naar dit adres (heap), met de klasse eromheen

Alle opdrachten van de module staan in agent/src/lib.rs (dispatch). `read` geeft max 64 KiB per keer;
`find_ptr` en `find_u16` stoppen bij hun limiet (find_ptr: 200 treffers).
"""
import json, os, socket, struct, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SOCK = os.path.expanduser("~/Library/Application Support/FM27 Trainer/agent.sock")

_proc = None


def _exe():
    if os.environ.get("FM27_EXE"):
        return os.environ["FM27_EXE"]
    name = "fm27-trainer.exe" if os.name == "nt" else "fm27-trainer"
    for sub in ("release", "debug"):
        p = os.path.join(REPO, "target", sub, name)
        if os.path.exists(p):
            return p
    raise RuntimeError("fm27-trainer niet gevonden: bouw eerst (cargo build -p fm27-trainer) of zet FM27_EXE")


def q(req):
    """Eén opdracht naar de module; geeft `data` terug of gooit een fout met de melding van de module."""
    global _proc
    if sys.platform == "darwin" and os.path.exists(SOCK):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(SOCK)
        s.sendall((json.dumps(req) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(1 << 20)
            if not chunk:
                break
            buf += chunk
        s.close()
        r = json.loads(buf)
    else:
        if _proc is None or _proc.poll() is not None:
            _proc = subprocess.Popen([_exe(), "agent-serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        _proc.stdin.write(json.dumps(req) + "\n")
        _proc.stdin.flush()
        r = json.loads(_proc.stdout.readline())
    if not r.get("ok"):
        raise RuntimeError(r.get("error"))
    return r["data"]


# ---- lezen -------------------------------------------------------------------------------------

def rd(addr, n):
    out = b""
    while n > 0:
        k = min(n, 1 << 16)
        out += bytes.fromhex(q({"cmd": "read", "addr": hex(addr), "len": k})["hex"])
        addr += k
        n -= k
    return out


def u64(a):
    return struct.unpack("<Q", rd(a, 8))[0]


def u32(a):
    return struct.unpack("<I", rd(a, 4))[0]


def i32(a):
    return struct.unpack("<i", rd(a, 4))[0]


def u16(a):
    return struct.unpack("<H", rd(a, 2))[0]


def cstr(a, n=96):
    return rd(a, n).split(b"\0")[0].decode("latin1")


def fmstr(p):
    """FM-string: u32 lengte + UTF-8 (soms via nog een pointer)."""
    for ptr in (p, None):
        try:
            if ptr is None:
                ptr = u64(p)
            n = u32(ptr)
            if 0 < n < 300:
                return rd(ptr + 4, n).decode("utf8", "replace")
        except Exception:
            pass
    return None


def is_ptr(v):
    return 0x10000 < v < 0x800000000000 and v % 8 == 0


def class_of(a):
    try:
        return q({"cmd": "class_of", "addr": hex(a)})
    except Exception:
        return None


def owner(a, back=0x400):
    """De klasse van het object waar adres `a` in ligt: de dichtstbijzijnde vtable ervoor."""
    for k in range(0, back, 8):
        c = class_of((a & ~7) - k)
        if c:
            return k + (a & 7), c
    return None


def describe(a, n=0x100):
    b = rd(a, n)
    rows = []
    for off in range(0, n, 8):
        v = struct.unpack_from("<Q", b, off)[0]
        lo, hi = struct.unpack_from("<ii", b, off)
        note = ""
        if is_ptr(v):
            c = class_of(v)
            note = "-> " + c if c else ("-> str %r" % fmstr(v) if fmstr(v) else "")
        rows.append("+%03x  %016x  i32=(%d, %d)  %s" % (off, v, lo, hi, note))
    print("\n".join(rows))


# ---- zoeken ------------------------------------------------------------------------------------

def find_ptr(target):
    """Plekken in de heap met precies deze 8 bytes (max 200; de eigen buffers van de module kunnen
    vals meetellen: controleer met u64(a) == target)."""
    return [int(a, 16) for a in q({"cmd": "find_ptr", "target": hex(target)}) or []]


def refs(target, back=0x400):
    out = []
    for a in find_ptr(target):
        if u64(a) == target:
            out.append((hex(a), owner(a, back)))
    return out


def find_u32(value, limit=400):
    hits = q({"cmd": "find_u16", "pattern": [value & 0xFFFF, value >> 16], "limit": limit})
    return [int(h, 16) for h in hits if int(h, 16) % 4 == 0]


def census(filt, count=False):
    """Klassen (RTTI) waarvan de naam `filt` bevat; met count=True ook het aantal objecten en de eerste adressen.
    Werkt op Windows (MSVC) en, na de fix van 8 nov 2026, ook op macOS."""
    return q({"cmd": "census", "filter": filt, "count": count})


def chain():
    c = q({"cmd": "chain"})
    return {k: int(v, 16) for k, v in c.items()}


def plist(header, max_n=500_000):
    """Een (begin, end, cap)-lijst van pointers."""
    b, e, c = struct.unpack("<QQQ", rd(header, 24))
    n = (e - b) // 8
    assert is_ptr(b) and b <= e <= c and n <= max_n, (hex(b), hex(e), hex(c))
    return list(struct.unpack("<%dQ" % n, rd(b, n * 8))) if n else []


if __name__ == "__main__":
    print(q({"cmd": "ping"}))
