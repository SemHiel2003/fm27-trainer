# FM27 Trainer: overdracht voor Claude

Lees dit eerst. Het vat alles samen wat in de eerste sessies (macOS, november 2026) is uitgezocht en gebouwd, zodat
je op een Windows-pc verder kunt. De gebruiker (Sem) is Nederlandstalig: antwoord in het Nederlands, kort en concreet.

## Wat het is

Een Rust-app (egui) voor Football Manager 27:

1. **Tactical Effectiveness uitlezen.** Schermafbeelding van FM's *Breakdown*, OCR (Apple Vision / Windows.Media.Ocr),
   taartmeters meten → 4 componenten + 17 key factors.
2. **Training plannen** op de zwakste gebieden, met de 59 sessies en 99 presets uit FM27's eigen data.
3. **Live in FM** (via een module): training in FM's kalender zetten en bijhouden (FM's assistent plant elke week de
   komende 1–2 weken opnieuw in; de app zet ze terug), budget, blessures, schorsingen, moraal, **spelers direct
   verkopen** aan een andere club en **tactisch begrip maximaliseren** (zoals FM's in-game editor).

Repo's (GitHub `SemHiel2003`):
- `fm27-trainer`: **openbaar**, releases v0.2.0 (genotariseerde Mac-DMG + Windows-zip), v0.2.1 en v0.2.2 (8 nov 2026,
  Windows-zip via GitHub Actions; de Mac-DMG komt van `./release.sh` op de Mac). Daar níét zomaar naar pushen; de dev-repo
  is er een voorouder van (fast-forward), en de release ontstaat door een tag `v*` naar de openbare repo te pushen (notes in
  `docs/releases/<tag>.md`). Lokaal heet die remote `public`.
- `fm27-trainer-dev`: **privé**, deze repo, met het nieuwste werk (verkopen, onderzoek biedingen, datumfix).

## Opbouw

```
app/                 de app (bin fm27-trainer): egui UI, OCR, planner, live-client
  src/main.rs        CLI: analyze, year, agent '<json>', agent-serve, train [--write], start-fm
  src/gui.rs         alle pagina's (Overzicht, Training, Planning, Club, Selectie, Verkopen, Instellingen)
  src/live.rs        verbindingstoestand, achtergrondtaken, automatisch training bijhouden (elke 8 s)
  src/live_training.rs  kalender lezen → plannen → per week schrijven
  src/agent.rs       macOS: socket naar de module; Windows: roept fm27_agent::request rechtstreeks aan
  src/planner.rs, model.rs, vision.rs, export.rs, theme.rs, i18n.rs (t!("nl", "en"))
agent/               crate fm27-agent: de spellogica, gedeeld
  src/lib.rs         dispatch van alle opdrachten (JSON); macOS: module in FM + socket-server
  src/mem.rs         geheugen: macOS (Mach, eigen proces), Windows (ReadProcessMemory/WriteProcessMemory op fm.exe),
                     tests (gesimuleerd geheugen)
  src/rtti.rs        klassen herkennen: Itanium (clang/macOS) en MSVC (Windows)
  src/game.rs        veldposities per platform (Layout MAC/WIN), builds (BUILDS), manager/club/selectie/training/undo
  src/sell.rs        speler direct verkopen
  src/understanding.rs  tactisch begrip (Tactical Understanding) per speler lezen en maximaliseren
  src/tests.rs       hele flow tegen gesimuleerd FM-geheugen, beide indelingen; MSVC-RTTI tegen testdata/rtti_test.dll
docs/TECHNICAL.md    architectuur en posities; docs/research/fm27-transfer-offers.md: onderzoek biedingen
tools/research/      fmq.py (onderzoeksclient), offers.py
```

Alle opdrachten van de module: `ping`, `club`, `squad`, `set_finances`, `heal`, `lift_bans`, `inspire`, `undo`,
`game_date`, `training_weeks`, `set_week`, `clubs`, `sell`, `understanding`, `max_understanding` (met `dry_run`),
`allow_untested`, en voor onderzoek `chain`, `census`, `class_of`, `read`, `find_u16`, `find_ptr`, `write` (alleen
met `expect`). Zie `agent/src/lib.rs`.

## Bouwen en testen op Windows

- Rust stable met de MSVC-toolchain (`rustup default stable-x86_64-pc-windows-msvc`) en Visual Studio Build Tools
  (workload "Desktop development with C++").
- `cargo test -p fm27-agent`: 7 tests, horen allemaal te slagen.
- `cargo run -p fm27-trainer`: de app. `cargo build --release -p fm27-trainer` → `target\release\fm27-trainer.exe`
  (statische C-runtime via `.cargo/config.toml`).
- **De release-exe heeft geen console** (`windows_subsystem = "windows"`). Gebruik voor de CLI de debug-build:
  `cargo run -p fm27-trainer -- agent "{\"cmd\":\"ping\"}"`. Voor scripts: `agent-serve` (stdin/stdout, werkt ook
  met de release-exe), zie `tools/research/fmq.py`.
- De Mac-DMG kan alleen op de Mac (`./release.sh`, Developer ID + notarisatie). Windows-zip: GitHub Actions
  (`.github/workflows/release.yml`, bij een tag `v*` of handmatig met `platform=windows`).
- Op deze pc (8 nov 2026 geïnstalleerd): rustup (`%USERPROFILE%\.cargo\bin`), VS Build Tools 2022 (MSVC 14.44,
  SDK 10.0.26100) en Python 3.12 (`%LOCALAPPDATA%\Programs\Python\Python312`); beide mappen staan op het
  gebruikers-PATH (een al open terminal ziet dat pas na een herstart). Python met `-X utf8` draaien, anders
  struikelt de console over accenten in spelersnamen.
- De agent-crate wordt ook in de debug-build geoptimaliseerd (`[profile.dev.package.fm27-agent]` in Cargo.toml):
  zonder dat duurt een heap-scan ruim een minuut in plaats van 1,5 s.

## Hoe live werkt op Windows

- De app zoekt `fm.exe` (Toolhelp), opent het met `PROCESS_VM_READ | VM_WRITE | VM_OPERATION | QUERY_INFORMATION`
  en leest/schrijft van buitenaf. FM gewoon via Steam starten; geen module of injectie.
- De native kern heet `game_plugin.dll`. Een build heet `PE-<linktijd>-<SizeOfImage>`; Steam 26340721 =
  `PE-6AE565AF-1821C000`. Onbekende build → geen vaste adressen (staflijst, clublijst, datum).
- **Schrijven staat op Windows aan** voor Steam-build 26340721 (`writes_tested: true` in `BUILDS`,
  `agent/src/game.rs`; live gecontroleerd op 8 nov 2026). Op een onbekende build (na een FM-patch) staat de app op
  alleen lezen; de gebruiker kan het per sessie toestaan (Instellingen → "Aanpassen op een niet-geteste FM-versie" →
  opdracht `allow_untested`).
- `game_plugin.dll` is **ingepakt**: secties heten `.xtext`, `.data1`, `.shared`, `.rodata`, `.sbss` (307 MB, wordt
  bij het laden gevuld), `.rdata` (klein). De MSVC-RTTI-zoeker doorzoekt daarom alle secties behalve
  bronnen/relocaties/debug/imports (`msvc_areas` in `agent/src/rtti.rs`); een blok dat niet in één keer leesbaar is,
  wordt in kleinere stukken geprobeerd (`mem::for_each_chunk`). `class_of` op het managerobject zelf geeft None
  (vbptr); de manager wordt via de staflijst gevonden. Een klasse opzoeken kost eenmalig ~1,5 s, een volledige
  heap-scan (3,5 GB) ~1,5 s in een release-build.

## Veiligheidsregels (altijd)

- Schrijven alleen via `write_checked`/`Patch`: eerst controleren dat het geheugen nog precies is wat je verwacht,
  daarna teruglezen. Elke wijziging met undo (`Change` in game.rs; geld als verschil).
- Experimenten alleen in een **aparte test-save**, en de gebruiker mag tijdens een experiment **niet doorspelen of
  opslaan**. Na een experiment: FM sluiten zonder op te slaan, of netjes terugzetten.
- Lijsten waarvan de plek iets betekent (de biedingenlijst: bodnummer = index) nooit opschuiven.
- `find_ptr` vindt ook de eigen buffers van de module: altijd controleren met `u64(a) == target`. Limiet 200.
- FMLibreEditor (github.com/tozmoz/FMLibreEditor) is GPL: gebruik hun **feiten** (posities, gedrag), kopieer geen code.
  Dit project is MIT.
- Niet naar de openbare repo pushen of releasen zonder het te vragen.

## Posities (Steam-build 26340721)

Spelers en staf verschillen per platform; clubs, financiën, contracten, teams, blessures en schorsingen niet.

| Veld | macOS | Windows |
|---|---|---|
| Manager: contract / identiteitsblok (+ shift 0x340) | 0x170 / 0x208 | 0x1A8 / 0x100 |
| Speler: team, blessure-record | 0x128, 0xF0 | 0x130, 0xF8 |
| Speler: scherpte, conditie, moraal (u8 1–20) | 0x250, 0x254, 0x264 | 0x258, 0x25C, 0x26C |
| Speler: namen (voor, achter, roep) | 0x298, 0x2A0, 0x2A8 | 0x2D8, 0x2E0, 0x2E8 |
| Speler: contract, identiteitsblok, UID | 0x2F0, 0x388, 0x394 | 0x330, 0x288, 0x294 |
| Speler: transferwaarde, vraagprijs (i32 pond) | 0x22C, 0x230 | 0x234, 0x238 |
| Speler: record tactisch begrip (ptr, zie onder) | 0xF8 (niet gemeten) | 0x100 |
| Interne persoons-id | identiteitsblok + 8 | identiteitsblok + 8 |
| Speler-trainer: alles vanaf het persoonsblok verder met | 0xF8 | 0xF8 |

Gedeeld: contract `+0x08` eigenaar (→ identiteitsblok), `+0x10` team, `+0x18` senior team, `+0x20` salaris,
`+0x26` job (1 speler), `+0x30` ontevredenheid (ptr), `+0x44/+0x48/+0x4C` getekend/einde/bij club (datum),
`+0x57` transfer-/huurlijst (bit 0/1), `+0x5A` happiness (**i8**), `+0x5D` rugnummer (255 = geen).
Team `+0x30` club, `+0x38` spelerslijst (begin, end, cap), `+0x88/+0x90` (vice-)aanvoerder.
Club `+0x0C` UID, `+0x18` teams (eerste = eerste elftal), `+0xB9` vrouwenclub, `+0xC0` naam, `+0x150` financiën.
Financiën `+0x0C` saldo, `+0x7C4` transferbudget, `+0x808` totaal loonbudget, `+0x814` loonbudget (i32 pond),
`+0x864` ticketprijs (f32), `+0x8A9` corporate (u8). Blessure-record: lijst huidige blessures (genezen = end := begin),
`+0x18` afwezigheden (schorsing: `0xFFFF` op +0x16). Datum = u16 dag-van-het-jaar (lage 9 bits, rest vlaggen) + u16 jaar.

Vaste plekken (RVA in game_plugin):

| | macOS arm64 | Windows |
|---|---|---|
| staflijst (begin, end) | 0x56D2E60 | 0x5705818 |
| clublijst (begin, end) | 0x56D2FD0 | 0x5705988 |
| spel-datum | 0x56B1DB8 | 0x56AF378 |

De manager wordt gevonden via de staflijst (Windows eerst) of RTTI `db::HUMAN_NON_PLAYER`.
**Spel-datum:** altijd de vaste plek; team+0xA0→+0x94 is een ándere datum (rond de laatste wedstrijd).

**Training** (gemeten op macOS én Windows, beide **+0x28**): een blok per team met de team-pointer en op +0x28 een
lijst van weekrecords (0x60 bytes: `+0x00` naam van het schema (FM-string), `+0x0C` maandag, `+0x10` 7 × [u16 0,
sessie 1, sessie 2, extra]). Op Windows probeert de code voor de zekerheid ook 0x08–0x80 met strenge controles,
voor het geval een latere build afwijkt. Sessiecodes: 0–7 Tactical Understanding, 100–108 Match Preparation, 200–228 Footballing
Development, 300–307 Physical, 400 Team Bonding, 401 Community, 402 reizen, 500 wedstrijd.

**Opstellingen**: `simatch::PREPARED_TACTICS_CREATOR_ARRAY`, `+0x08` lijst, `+0x28` 26 × u32 persoons-id
(0xFFFFFFFF = leeg). Lijstblokken van de allocator: Mac 144…4096, Windows 144…512 (`heap_blocks`).

**Tactisch begrip** (FM27's "Tactical Understanding", de opvolger van tactical familiarity; gevonden op 8 nov 2026
door FM's editor-actie `TEAM_MAXIMISE_TACTICAL_UNDERSTANDING_ACTION` te disassembleren, zie
`agent/src/understanding.rs`): speler `+0x100` → record van 0xE0 bytes. Zes blokken van 0x20 (gebied 0–5: Passing
Style, Build Up, Chance Creation, In Possession Shape, Line of Engagement, Pressing Intensity): (begin, end, cap)-lijst
van u32-items (bits 0–13 niveau 0–10000, bits 14–19 tactiekinstelling 0–20, rest rommel) + u64 gebiedsnummer op
`+0x18`; op `+0xC0` een lijst van 16-byte-items voor Pressing Trap en Out Of Possession Shape (u16 rol/positie, 11
bytes vorm, u8 vlaggen, u16 niveau op `+0x0E`); `+0xD8` datum, `+0xDC` "veranderd". Begrip voor een instelling = max
over items van (niveau − 1000 × afstand), bodem 1000. Maximaliseren zet elk item op 10000 en voegt de meest
voorkomende instelling toe voor wie hem mist, alleen binnen de capaciteit van FM's lijst (geen eigen allocatie).
Live getest op Windows (8 nov 2026, Ajax-save, 27 spelers). Het trainingsblok van TRAINING_MANAGER per team (zie
boven) en `TACTICS_MANAGER` bevatten dit níét. Disassembleren kan met capstone (`pip install capstone`) op code die
via `read` uit fm.exe komt; game_plugin.dll ligt op Windows ingepakt op schijf maar uitgepakt in het geheugen.

## Wat is getest, waar

| | macOS (live, Ajax-save) | Windows |
|---|---|---|
| uitlezen club/selectie/kalender | ✅ | ✅ (8 nov 2026, Ajax-save) |
| budget, genezen, moraal, training schrijven, undo | ✅ | ✅ (8 nov 2026) |
| verkopen (`sell`) | alleen simulatie | ✅ dry run én één echte verkoop + undo (8 nov 2026) |
| tactisch begrip (`understanding`, `max_understanding`) | alleen simulatie (offset 0xF8 is een gok) | ✅ gelezen en gemaximaliseerd (8 nov 2026) |
| MSVC-RTTI | n.v.t. | ✅ testdata én live (census, opstellingen) |

## Taak 1: Windows live controleren — gedaan (8 nov 2026)

Alle stappen hieronder zijn op Steam-build 26340721 doorlopen en groen: lezen klopte met FM (club, saldo, budgetten,
datum, selectie, kalender), en `set_finances`, `inspire`, `heal`, `set_week` en `sell` (dry run én één echte
verkoop) werkten elk met `undo`. Gevonden en opgelost: de RTTI-zoeker keek alleen in `.data`/`.rdata` (ingepakte
DLL, zie boven) waardoor verkopen faalde, en de debug-build was ~40x te traag. Na een FM-patch (build onbekend:
`known_build false`) dezelfde procedure opnieuw doen.

Gebruik een test-save. Start FM27 via Steam, laad de save, dan:

```
cargo build -p fm27-trainer
python -i tools\research\fmq.py
>>> q({"cmd":"ping"})            # build moet PE-6AE565AF-1821C000 zijn, known_build false
>>> q({"cmd":"club"})            # naam, saldo, budgetten: vergelijk met FM (FM toont euro's: pond × ~1,17)
>>> q({"cmd":"squad"})           # namen, moraal, conditie, blessures, waarde, contract
>>> q({"cmd":"game_date"})       # moet de datum in FM zijn
>>> q({"cmd":"training_weeks"})  # maandagen 7 dagen uit elkaar, codes zoals FM's kalender
```

Klopt alles, sta schrijven toe (`q({"cmd":"allow_untested","on":True})`) en test, telkens met `undo` erachteraan en
controle in FM: `set_finances` (transferbudget +1), `inspire` voor één speler, `heal`, `set_week` voor één dag in de
toekomst, `sell` met `dry_run`, daarna één echte verkoop + `undo`. Werkt alles: zet `writes_tested: true` voor de
Windows-build in `BUILDS`, noteer in welke afstand het trainingsblok gevonden werd (`State.training_block`; zo
nodig `TB_WEEKS` per platform maken), werk docs/TECHNICAL.md en de README bij, en vraag of er een release moet komen.

Gaat iets mis: kijk met `census`, `describe`, `refs` waar het verschilt; vergelijk met FMLibreEditor's
`profiles/fm27/layout.toml`.

## Taak 2: transferbiedingen (zie docs/research/fm27-transfer-offers.md)

De wens van de gebruiker: in de app zeggen "verkoop speler X aan club Y voor Z" en dan komt er in FM een **echt bod**.

Bekend (macOS): een bod is `FULL_TRANSFER_OFFER` (0xD0 bytes, uit een pool), alle biedingen staan in
`TRANSFER_MANAGER+0x08` (bodnummer = index). FM toont een bod alleen via zijn inbox-bericht `TRANSFER_OFFER_NEWS`
(groot object, ~40 verwijzingen, in een nieuwsstroom zonder lijst) en een detailrecord met totaalsom, bodnummer,
volgnummer en bonussen. Een bod zonder bericht is onzichtbaar en wordt niet afgehandeld (getest).

Opties, met de gebruiker gekozen route nog open:
- **A. Een binnenkomend bod ombouwen** (koper, speler, som) in bod + detailrecord + beide kopieën van het bericht,
  daarna in FM accepteren → FM doet een echte transfer. Volgende stap: met een vers bod in een test-save alle velden
  van het bericht en het detailrecord in kaart brengen (op Windows eerst de offsets van het bod controleren:
  `tools/research/offers.py`).
- **B. Direct verkopen** (gebouwd, `sell`): betrouwbaar maar zonder bericht/historie.
- **C. FM zelf laten bieden**: transferlijst + vraagprijs + koper meer budget geven.

FM27 gebruikt nepnamen voor clubs zonder licentie: AC Milan = **Casciavit Milano** (UID 1099), Inter =
**Blu-neri Milano** (UID 1135). Handig voor het zoekveld in de app (aliassen?).

## Schermuitlezing (Breakdown) op Windows, gemeten 8 nov 2026

- Elke taartmeter heeft **10 segmenten**, gevuld met de klok mee vanaf 12 uur; het lege deel is **bijna wit**
  (s < 0,25, v > 0,66), de spaken zijn ook licht, de achtergrond van een gemarkeerde rij is middelgrijs
  (s ≈ 0,12, v ≈ 0,34) en viel vroeger onder "meter-grijs". De score is nu het aantal gevulde segmenten × 10
  (laatste echt gekleurde sector, afgerond op een segment); de tint (oranje laag, geel midden, groen hoog) wordt
  alleen bewaard. Balken: 11 plekken, één gekleurde (de stand) en één witte streep; stand = plek vanaf het
  eerste streepje / 10. De doorschijnende paneelachtergrond kan een deel van de streepjes in ruis zetten.
- Windows-OCR leest "Player Buy-In" als "Player Buy-ln" (alias in `model.rs`).
- **Resolutie-onafhankelijk sinds 8 nov 2026** (melding van een gebruiker: "Read 8 of 17 gauges", key factors
  op 0 of 50): `analyze_image` leest eerst op ware grootte en, als de labels niet ~20 px hoog zijn (`REF_TEXT`,
  de maat van 2560×1440 op 100 %), nog eens op een kopie die naar die maat is geschaald (zonder enig herkend
  label: 1,5× en 2×); de meting op de referentiemaat wint bij gelijke stand. Verder: labels worden tolerant
  gematcht (Levenshtein, ~1 letter per 6), een gemist label krijgt zijn rij uit de rij-afstand en de
  meterkolom van de gevonden labels, en een kolom zonder enig label uit de kop "Key Factors" (eerste rij
  2,15 × kophoogte eronder, meter = laatste ronde blok op de rij). Elke taartmeter wordt vóór het meten naar
  72 px gebracht; de vulling is de aaneengesloten reeks gekleurde sectoren vanaf 12 uur (één sector gat =
  spaak). De balkmeters worden gelezen als helderheidspieken (streepjes) met de kaderrand eruit (kolommen die
  over meer dan 60 % van de hoogte licht zijn); de stand = plek van de gekleurde streep tussen eerste en laatste
  streepje × 10. Getest door `last-breakdown.png` met Pillow te verkleinen naar 3840×2160 … 1280×720: alle 21
  waarden gelijk van 1366×768 tot 4K; op 720p (sterk vervaagd) 15 van 17. `FM27_VISION_DEBUG=1` print de
  balkmeting.
- Elke uitlezing bewaart `last-capture.png` en, als er een Breakdown op stond, `last-breakdown.png` in
  `%APPDATA%\FM27 Trainer`; naspelen met `target\debug\fm27-trainer.exe analyze <png> --ocr`. Hulpscripts om
  pixels te bekijken stonden in de scratchpad van de sessie (pie_debug.py, montage.py); zo nodig opnieuw maken
  met Pillow (`python -m pip install --user pillow`).

## Overig

- Ondertekenen Mac: Developer ID "Sem Hiel (SFV4V5SHM4)", notarytool-profiel `fm27-notary` (alleen op de Mac).
- De Windows-exe is niet ondertekend (SmartScreen: "Meer info → Toch uitvoeren").
- Testhulpen GUI: `FM27_PAGE=training|planning|club|squad|sell|settings`, `FM27_LANG=nl|en`,
  `FM27_SNAPSHOT=pad.png` (schermafbeelding van het eigen venster, dan afsluiten), `FM27_WINDOW=1320x1000`.
  Met `FM27_SNAPSHOT` slaat de app het automatisch bijhouden van training over (sinds 8 nov 2026): een testopname
  schrijft nooit in FM. Zonder die variabele schrijft een gestarte app met "automatisch bijhouden" aan binnen
  enkele seconden het trainingsplan in FM; houd daar rekening mee bij tests tegen een echte save.
