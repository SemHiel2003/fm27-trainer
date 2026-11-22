# FM27 Trainer on Linux (Steam Proton)

FM27 runs on Linux through Steam's Proton. FM27 Trainer has no Linux build, but the **Windows build runs
inside FM27's own Proton session** and then reaches `fm.exe` exactly as it does on Windows: connecting,
training into FM's calendar and the club tools work as on Windows.

Two things Windows provides are missing under Proton: capturing FM's window (FM draws through Vulkan, so the
capture comes back black) and Windows' text recognition (Windows.Media.Ocr). A small helper on the Linux side,
`tools/linux/fm27-ocr-bridge`, does both (screen capture plus Tesseract), so **Read FM screen** works too.

Tested on Arch Linux with Hyprland, Steam build 26620374 (17 November 2026), Proton Experimental, 2560×1440.

## What you need

- FM27 from Steam, run with Proton.
- `tesseract` with English data, ImageMagick 7 (`magick`), and for the capture: `grim` on Hyprland or sway,
  or `xdotool` + ImageMagick's `import` on X11. Arch: `pacman -S tesseract tesseract-data-eng imagemagick grim`.
- On other Wayland desktops (GNOME, KDE) capturing FM's window is not supported yet: take a screenshot of the
  Breakdown yourself and use **From file** in the app. Everything else works.

## Install

1. Download [FM27-Trainer-Windows.zip](../../../releases/latest/download/FM27-Trainer-Windows.zip) and unzip
   `FM27 Trainer.exe` into FM27's Proton prefix, at `drive_c/FM27Trainer/`:
   `<Steam library>/steamapps/compatdata/4530370/pfx/drive_c/FM27Trainer/FM27 Trainer.exe`.
   (Elsewhere in the prefix works too: set `FM27_TRAINER_EXE` to its Windows path, e.g.
   `C:\Games\FM27 Trainer.exe`.)
2. Put the three scripts from `tools/linux/` somewhere permanent and make them executable.
3. In Steam: Football Manager 27 → Properties → General → **Launch Options**:

   ```
   /path/to/tools/linux/fm27-launch %command%
   ```

   If you already have launch options (for example `WINEDLLOVERRIDES=...` for a BepInEx mod), keep them in
   front: `WINEDLLOVERRIDES="version=n,b" /path/to/tools/linux/fm27-launch %command%`.

That's it: start FM27 from Steam and FM27 Trainer opens alongside it, and closes when FM closes.

## How it works

- `fm27-launch` sets `STEAM_COMPAT_LAUNCHER_SERVICE=proton`, which makes Proton start Steam's launcher service
  inside FM27's container. It also starts the helper and `fm27-trainer --wait`, then starts FM27 as usual.
- `fm27-trainer` waits for `fm.exe`, then runs the exe in FM27's session with
  `steam-runtime-launch-client --inside-app=4530370 -- wine "C:\FM27Trainer\FM27 Trainer.exe"`. Both then share
  one wineserver, so `OpenProcess`/`ReadProcessMemory`/`WriteProcessMemory` on `fm.exe` work as on Windows.
  You can also start it by hand while FM27 runs.
- `fm27-ocr-bridge serve` refreshes `host.alive` in `%APPDATA%\FM27 Trainer\linux-bridge` every 2 s while FM27
  runs. While that file is fresh, the app sends its capture and OCR requests there (`wine_bridge` in
  `app/src/vision.rs`) instead of using xcap and Windows.Media.Ocr. On Windows the folder never exists, so
  nothing changes there. The helper exits when FM27 does and closes the trainer, so Steam doesn't keep FM27
  "running".
- The capture brings FM's window to the front if it isn't visible, and gives focus back afterwards.
- OCR: the capture is made grey, scaled 2×, contrast-stretched and inverted, then read by Tesseract
  (`--psm 3`). Words far apart on one line are split into separate lines, as Windows' OCR does (otherwise a
  title gets joined to the bar at the other end of its panel).

Testing: `fm27-ocr-bridge capture out.png` captures FM's window; `fm27-ocr-bridge ocr out.png` prints what the
OCR reads. Logs: `~/.cache/fm27-ocr-bridge.log` and `~/.cache/fm27-trainer.log`.

## Limits

- English OCR only (Tesseract `eng`). FM in another language: install that Tesseract language and change `-l eng`
  in `fm27-ocr-bridge`.
- Steam Deck: not tested. The scripts need a desktop session (Desktop Mode) for the capture.
