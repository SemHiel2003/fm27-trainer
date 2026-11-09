"""Regressietest voor de schermuitlezing: maakt varianten van een Breakdown-opname (andere schermgroottes,
donkerder/lichter zoals HDR of een nachtlampje, andere paneelkleuren, de labels overgeschilderd in een andere
taal van FM) en vergelijkt de uitlezing van elke variant met die van de opname zelf.

    python -X utf8 tools/research/vision_regress.py "%APPDATA%/FM27 Trainer/last-breakdown.png" [exe] [map]

Nodig: `pip install pillow numpy`, een build van de app (standaard target/debug/fm27-trainer.exe) en Windows-OCR
(of macOS). De opname moet goed gelezen worden (21 waarden); de varianten horen dezelfde waarden te geven.
Bekend fout (9 nov 2026): zeer verzadigde groene/gele paneelrijen, grijs of met verwisselde kleurkanalen.
"""
import os, re, subprocess, sys, tempfile

import numpy as np
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))


def read(exe, png):
    """(17 meters, 4 balken) zoals de app ze leest; None voor wat ontbreekt"""
    out = subprocess.run([exe, "analyze", png, "--ocr"], capture_output=True, text=True, encoding="utf-8").stdout
    gauges, bars, ocr = {}, {}, []
    for line in out.splitlines():
        if m := re.match(r"meter (.+?)\s+(-?[\d.]+)\s+vulling", line):
            gauges[m.group(1).strip()] = round(float(m.group(2)))
        elif m := re.match(r"balk\s+(.+?)\s+(-?[\d.]+)$", line):
            bars[m.group(1).strip()] = round(float(m.group(2)))
        elif m := re.match(r"OCR\s+(-?\d+),\s*(-?\d+)\s+(-?\d+),\s*(-?\d+)  (.*)", line):
            ocr.append((*map(int, m.groups()[:4]), m.group(5)))
    return gauges, bars, ocr


def labels():
    """taal -> (titels, factors, key factors) uit app/src/labels.rs"""
    src = open(os.path.join(ROOT, "app", "src", "labels.rs"), encoding="utf-8").read()
    out = {}
    for block in re.findall(r"Lang \{(.*?)\n    \}", src, re.S):
        name = re.search(r'name: "(.*?)"', block).group(1)
        comps = re.findall(r'"((?:[^"\\]|\\.)*)"', re.search(r"components: \[(.*?)\]", block, re.S).group(1))
        facts = re.findall(r'"((?:[^"\\]|\\.)*)"', re.search(r"factors: \[(.*?)\]", block, re.S).group(1))
        kf = re.search(r'key_factors: "(.*?)"', block).group(1)
        out[name] = (comps, facts, kf)
    return out


def paint(src, ocr, lang, en, path):
    """de Engelse labels overschilderen met die van `lang` (lettertype Segoe UI, zoals FM ongeveer)"""
    img = src.copy()
    d = ImageDraw.Draw(img)
    comps, facts, kf = lang
    pairs = [(e, t, True) for e, t in zip(en[0], comps)] + [(e, t, False) for e, t in zip(en[1], facts)] + [(en[2], kf, False)]
    font_dir = os.path.join(os.environ.get("WINDIR", "C:/Windows"), "Fonts")
    for e, t, title in pairs:
        key = e if e != "Player Buy-In" else "Player Buy-"
        for x0, y0, x1, y1, txt in ocr:
            i = txt.find(key)
            if i < 0 or (not title and e != en[2] and len(txt) > len(e) + 6):
                continue
            bx0 = x0 + (x1 - x0) * i / len(txt)
            bg = src.getpixel((min(int(x1) + 6, src.width - 1), (y0 + y1) // 2))
            d.rectangle([bx0 - 2, y0 - 4, x1 + 4, y1 + 5], fill=bg)
            h = y1 - y0
            size = int(h * (1.35 if title else 1.2))
            face = "segoeuib.ttf" if title else "segoeui.ttf"
            font = ImageFont.truetype(os.path.join(font_dir, face), size)
            # past de tekst niet vóór de meter, dan kleiner (zoals FM's automatische tekstgrootte)
            room = (x1 - bx0) + 8 * h
            while d.textlength(t, font=font) > room and size > 8:
                size -= 1
                font = ImageFont.truetype(os.path.join(font_dir, face), size)
            d.text((bx0, (y0 + y1) / 2), t, fill=(236, 236, 236), font=font, anchor="lm")
    img.save(path)


def main():
    png = sys.argv[1]
    exe = os.path.abspath(sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, "target", "debug", "fm27-trainer.exe"))
    out = sys.argv[3] if len(sys.argv) > 3 else tempfile.mkdtemp(prefix="fm27-vision-")
    os.makedirs(out, exist_ok=True)
    ref_g, ref_b, ocr = read(exe, png)
    print(f"opname: {len(ref_g)} meters, {len(ref_b)} balken; varianten in {out}")
    if not ref_g:
        sys.exit("geen meters gelezen: is dit een build met `analyze --ocr` in het formaat van last-analysis.txt (vanaf 0.2.4)?")
    if len(ref_g) < 17 or len(ref_b) < 4:
        print("let op: de opname zelf wordt niet volledig gelezen; de vergelijking is dan minder waard")
    src = Image.open(png).convert("RGB")
    a = np.asarray(src).astype(np.float32) / 255
    v = a.max(axis=2, keepdims=True)
    lin = np.where(a <= 0.04045, a / 12.92, ((a + 0.055) / 1.055) ** 2.4)
    tone = {
        "donker70": a * 0.7,
        "lineair": lin,  # lineair licht als sRGB bewaard (HDR-opname): te donker
        "gamma06": a ** 0.6,  # te licht
        "opgetild30": 0.3 + 0.7 * a,  # opgetilde zwarttinten
        "flets50": 0.5 * a + 0.5 * a.mean(axis=2, keepdims=True),
        "panelen_lichter": np.where(v < 0.45, a + 0.18, a),
        "panelen_blauw": np.where(v < 0.45, a * [0.6, 0.8, 1.6], a),
    }
    variants = []
    for name, arr in tone.items():
        p = os.path.join(out, name + ".png")
        Image.fromarray((np.clip(arr, 0, 1) * 255).round().astype(np.uint8)).save(p)
        variants.append(p)
    for w, h in [(1366, 768), (1920, 1080), (3440, 1440), (3840, 2160)]:
        p = os.path.join(out, f"formaat_{w}x{h}.png")
        src.resize((w, h), Image.LANCZOS).save(p)
        variants.append(p)
    labs = labels()
    for lang in ["german", "dutch", "french", "spanish", "polish", "turkish", "english (us)"]:
        p = os.path.join(out, "taal_" + lang.replace(" ", "_").replace("(", "").replace(")", "") + ".png")
        paint(src, ocr, labs[lang], labs["english"], p)
        variants.append(p)
    bad = 0
    for p in variants:
        g, b, _ = read(exe, p)
        # in US English heten twee factors anders, maar de app noemt ze altijd met de Engelse naam
        diff = [f"{k} {ref_g[k]}→{g.get(k)}" for k in ref_g if g.get(k) != ref_g[k]]
        diff += [f"[{k}] {ref_b[k]}→{b.get(k)}" for k in ref_b if b.get(k) != ref_b[k]]
        bad += bool(diff)
        print(("OK   " if not diff else "FOUT ") + os.path.basename(p) + ("" if not diff else ": " + "; ".join(diff)))
    print(f"{len(variants) - bad} van {len(variants)} goed")


if __name__ == "__main__":
    main()
