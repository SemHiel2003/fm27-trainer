# FM27 Trainer: overdracht voor Claude

Lees dit eerst. Het vat alles samen wat in de eerste sessies (macOS, november 2026) is uitgezocht en gebouwd, zodat
je op een Windows-pc verder kunt. De gebruiker (Sem) is Nederlandstalig: antwoord in het Nederlands, kort en concreet.

## Wat het is

Een Rust-app (egui) voor Football Manager 27:

1. **Tactical Effectiveness uitlezen.** Schermafbeelding van FM's *Breakdown*, OCR (Apple Vision / Windows.Media.Ocr),
   taartmeters meten → 4 componenten + 17 key factors.
2. **Training plannen** op de zwakste gebieden, met de 59 sessies en 99 presets uit FM27's eigen data.
3. **Live in FM** (via een module): training in FM's kalender zetten en bijhouden (FM's assistent plant elke week de
   komende 1–2 weken opnieuw in; de app zet ze terug), budget, blessures, schorsingen, moraal, en **spelers direct
   verkopen** aan een andere club.

Repo's (GitHub `SemHiel2003`):
- `fm27-trainer`: **openbaar**, release v0.2.0 (genotariseerde Mac-DMG + Windows-zip). Daar níét zomaar naar pushen.
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
  src/tests.rs       hele flow tegen gesimuleerd FM-geheugen, beide indelingen; MSVC-RTTI tegen testdata/rtti_test.dll
docs/TECHNICAL.md    architectuur en posities; docs/research/fm27-transfer-offers.md: onderzoek biedingen
tools/research/      fmq.py (onderzoeksclient), offers.py
```

Alle opdrachten van de module: `ping`, `club`, `squad`, `set_finances`, `heal`, `lift_bans`, `inspire`, `undo`,
`game_date`, `training_weeks`, `set_week`, `clubs`, `sell`, `allow_untested`, en voor onderzoek `chain`, `census`,
`class_of`, `read`, `find_u16`, `find_ptr`, `write` (alleen met `expect`). Zie `agent/src/lib.rs`.

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

## Hoe live werkt op Windows

- De app zoekt `fm.exe` (Toolhelp), opent het met `PROCESS_VM_READ | VM_WRITE | VM_OPERATION | QUERY_INFORMATION`
  en leest/schrijft van buitenaf. FM gewoon via Steam starten; geen module of injectie.
- De native kern heet `game_plugin.dll`. Een build heet `PE-<linktijd>-<SizeOfImage>`; Steam 26340721 =
  `PE-6AE565AF-1821C000`. Onbekende build → geen vaste adressen (staflijst, clublijst, datum).
- **Schrijven staat op Windows uit** (`writes_tested: false` in `BUILDS`, `agent/src/game.rs`), tot iemand het live
  heeft gecontroleerd. De gebruiker kan het per sessie toestaan (Instellingen → "Aanpassen op een niet-geteste
  FM-versie" → opdracht `allow_untested`).

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

**Training** (alleen op macOS gemeten): een blok per team met de team-pointer en op **+0x28** een lijst van weekrecords
(0x60 bytes: `+0x00` naam van het schema (FM-string), `+0x0C` maandag, `+0x10` 7 × [u16 0, sessie 1, sessie 2,
extra]). Op Windows probeert de code eerst 0x28, dan 0x08–0x80 met strenge controles; **de echte afstand op Windows
is nog niet gemeten**. Sessiecodes: 0–7 Tactical Understanding, 100–108 Match Preparation, 200–228 Footballing
Development, 300–307 Physical, 400 Team Bonding, 401 Community, 402 reizen, 500 wedstrijd.

**Opstellingen**: `simatch::PREPARED_TACTICS_CREATOR_ARRAY`, `+0x08` lijst, `+0x28` 26 × u32 persoons-id
(0xFFFFFFFF = leeg). Lijstblokken van de allocator: Mac 144…4096, Windows 144…512 (`heap_blocks`).

## Wat is getest, waar

| | macOS (live, Ajax-save) | Windows |
|---|---|---|
| uitlezen club/selectie/kalender | ✅ | alleen simulatie + CI-build |
| budget, genezen, moraal, training schrijven, undo | ✅ | alleen simulatie |
| verkopen (`sell`) | alleen simulatie | alleen simulatie |
| MSVC-RTTI | n.v.t. | ✅ tegen een echte MSVC-DLL (testdata) |

## Taak 1: Windows live controleren (prioriteit)

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

## Overig

- Ondertekenen Mac: Developer ID "Sem Hiel (SFV4V5SHM4)", notarytool-profiel `fm27-notary` (alleen op de Mac).
- De Windows-exe is niet ondertekend (SmartScreen: "Meer info → Toch uitvoeren").
- Testhulpen GUI: `FM27_PAGE=training|planning|club|squad|sell|settings`, `FM27_LANG=nl|en`,
  `FM27_SNAPSHOT=pad.png` (schermafbeelding van het eigen venster, dan afsluiten), `FM27_WINDOW=1320x1000`.
