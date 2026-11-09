"""Haalt de labels van FM27's Breakdown-scherm (Tactical Effectiveness) uit FM's eigen taalbestanden en
schrijft app/src/labels.rs, zodat de schermuitlezing in elke taal van FM werkt.

    python -X utf8 tools/research/breakdown_labels.py "<FM27>/data/game_simulation/languages.fmf"

Nodig: `pip install zstandard`.

languages.fmf is een SI-archief (zie tools/fmf_unpack.py) met per taal een .ltc-bestand:
  "\\x03\\x01ctl." u16 ? u32 ? u32 ?, u32 lengte + naam van de taal, u32 ?,
  dan de teksten achter elkaar: u8 n (aantal varianten, bv. mannelijk/vrouwelijk), n × (u32 lengte, UTF-8),
  dan een tabel: u32 aantal, aantal × (u32 tekst-id, u32 plek van de tekst). De id's zijn in elke taal gelijk.
"""
import io, os, struct, sys

import zstandard

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "..", "app", "src", "labels.rs")

# tekst-id's van de Breakdown (gevonden via de Engelse teksten, allemaal in het blok 0x100AE1xx)
COMPONENTS = [0x100AE138, 0x100AE11A, 0x100AE118, 0x100AE11C]
FACTORS = [0x100AE136, 0x100AE127, 0x100AE128, 0x100AE137, 0x100AE132, 0x100AE135, 0x100AE134, 0x100AE133,
           0x100AE12D, 0x100AE12C, 0x100AE295, 0x100AE11F, 0x100AE120, 0x100AE124, 0x100AE13B, 0x100AE12F, 0x100AE12E]
# "Key Factors" heeft geen eigen id in de tabel: het is de tekst vlak vóór "Lineup" (0x100AE10B)
BEFORE_KEY_FACTORS = 0x100AE10B

# taal van het .ltc-bestand (en een taalcode, ter informatie); Engels eerst (wint bij gelijke stand)
LANGS = [("english", "en-GB"), ("english (us)", "en-US"), ("dutch", "nl"), ("german", "de"), ("french", "fr"),
         ("spanish", "es"), ("spanish (latam)", "es-MX"), ("italian", "it"), ("portuguese", "pt"), ("danish", "da"),
         ("norwegian", "nb"), ("swedish", "sv"), ("polish", "pl"), ("turkish", "tr"), ("indonesian", "id"),
         ("greek", "el"), ("russian", "ru"), ("chinese", "zh-Hans"), ("japanese", "ja"), ("korean", "ko")]
# Arabisch ontbreekt bewust: FM spiegelt de schermen dan waarschijnlijk (rechts naar links) en dat is niet getest.


def zstd(b):
    return zstandard.ZstdDecompressor().stream_reader(io.BytesIO(b), read_across_frames=True).read()


def rd_str(m, p):
    n = struct.unpack_from("<I", m, p)[0]
    return m[p + 4:p + 4 + n].decode("utf-8", "replace"), p + 4 + n


def rd_dir(m, p, path, out):
    nf = struct.unpack_from("<I", m, p)[0]; p += 4
    for _ in range(nf):
        name, p = rd_str(m, p); ext, p = rd_str(m, p)
        off, size, raw, _, _ = struct.unpack_from("<QQQqq", m, p); p += 40
        out[path + name + ext] = (off, size)
    nd = struct.unpack_from("<I", m, p)[0]; p += 4
    for _ in range(nd):
        name, p = rd_str(m, p); p = rd_dir(m, p, path + name + "/", out)
    return p


def unpack(fn):
    d = open(fn, "rb").read()
    assert d[2:6] == b"fmf.", "versleuteld archief: zie tools/fmf_unpack.py"
    a = struct.unpack_from("<Q", d, 9)[0]
    m = zstd(d[a + 18:])
    _, p = rd_str(m, 0)
    files = {}
    rd_dir(m, p, "", files)
    return {name: (lambda o=off, s=size: zstd(d[26 + o:26 + o + s])) for name, (off, size) in files.items()}


def parse_ltc(d):
    p = 16
    n = struct.unpack_from("<I", d, p)[0]; p += 4 + n + 4
    offs, texts = [], []
    while True:
        t = d[p]
        assert t > 0, p
        q, first = p + 1, None
        for _ in range(t):
            s, q = rd_str(d, q)
            first = s if first is None else first
        offs.append(p); texts.append(first); p = q
        # de tabel: het aantal teksten, en de eerste plek wijst naar de eerste tekst (plekken tellen vanaf byte 8)
        if p + 12 <= len(d) and struct.unpack_from("<I", d, p)[0] == len(offs) and struct.unpack_from("<I", d, p + 8)[0] + 8 == offs[0]:
            break
    base = 8
    index = {o: i for i, o in enumerate(offs)}
    ids = {}
    for k in range(len(offs)):
        sid, off = struct.unpack_from("<II", d, p + 4 + 8 * k)
        ids[sid] = index.get(off + base)
    return texts, ids


def clean(s):
    return s.replace("​", "").strip()  # het Chinees heeft zero-width spaces tussen de tekens


def rust(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    files = unpack(sys.argv[1])
    rows = []
    for lang, tag in LANGS:
        texts, ids = parse_ltc(files[f"{lang}.ltc"]())
        get = lambda i: clean(texts[ids[i]])
        comps = [get(i) for i in COMPONENTS]
        facts = [get(i) for i in FACTORS]
        kf = clean(texts[ids[BEFORE_KEY_FACTORS] - 1])
        if lang == "english":
            assert comps[0] == "Tactical Understanding" and facts[-1] == "Opposition Player Knowledge" and kf == "Key Factors", (comps, facts, kf)
        rows.append((lang, tag, comps, facts, kf))
        print(f"{lang:16} {tag:8} {comps[0]} | {facts[2]} | {kf}")
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("//! De labels van het Breakdown-scherm in elke taal van FM27, letterlijk uit FM's eigen taalbestanden\n")
        f.write("//! (`data/game_simulation/languages.fmf`). Gegenereerd door `tools/research/breakdown_labels.py`; niet\n")
        f.write("//! met de hand aanpassen. Volgorde: `Component::ALL` en `Factor::ALL`.\n\n")
        f.write("pub struct Lang {\n    /// taal in FM\n    pub name: &'static str,\n")
        f.write("    pub components: [&'static str; 4],\n    pub factors: [&'static str; 17],\n    /// kop boven elke kolom (\"Key Factors\")\n    pub key_factors: &'static str,\n}\n\n")
        f.write("pub const LANGS: &[Lang] = &[\n")
        for lang, tag, comps, facts, kf in rows:
            f.write(f"    Lang {{\n        name: {rust(lang)},\n")
            f.write("        components: [" + ", ".join(rust(c) for c in comps) + "],\n")
            f.write("        factors: [\n" + "".join(f"            {rust(x)},\n" for x in facts) + "        ],\n")
            f.write(f"        key_factors: {rust(kf)},\n    }},\n")
        f.write("];\n")
    print("geschreven:", os.path.normpath(OUT))


if __name__ == "__main__":
    main()
