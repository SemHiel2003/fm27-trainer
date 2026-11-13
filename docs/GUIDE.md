# FM27 Trainer: getting started

How to install the app, start it the right way and what to do when it does not connect.
Download the latest version here: [Windows](https://github.com/SemHiel2003/fm27-trainer/releases/latest/download/FM27-Trainer-Windows.zip) ·
[macOS](https://github.com/SemHiel2003/fm27-trainer/releases/latest/download/FM27-Trainer-macOS.dmg).

The app only works with **single-player** saves, and FM has to run on the **same computer** as the app.

## Windows

1. Unzip `FM27-Trainer-Windows.zip` to a normal folder (not inside the zip) and run **FM27 Trainer.exe**.
   The exe is not code-signed: if SmartScreen warns, click *More info → Run anyway*.
2. Start FM27 the way you always do: Steam, the Xbox app / Game Pass or Epic.
3. **Load your save.** The app can only connect once you are inside a save, not on FM's start screen.
4. The app connects by itself within a few seconds; the sidebar shows *Connected to FM27*. If it does not, click
   *Reconnect*.

The order does not matter on Windows: you can start the app first or FM first. Nothing is installed into FM.

## macOS

On a Mac the order **does** matter, because the app has to start FM itself.

1. Open the DMG and drag **FM27 Trainer** to *Applications*.
2. On first launch, allow **Screen Recording** (System Settings → Privacy & Security). The app needs it to read
   the Breakdown screen. Restart the app after allowing it.
3. **Save your game and quit FM27 completely.** If FM is already running, the app cannot attach to it.
4. Make sure Steam is open, then click **Start FM with module** in the app.
5. Load your save. The app connects by itself.

If you start FM from Steam yourself, reading the Breakdown and planning still work, but the live features
(training into FM's calendar, budgets, morale, transfers) do not.

## First use

1. In FM, open *Tactical Effectiveness → Breakdown*.
2. In the app, press **Read FM screen** on the Overview page. Check the values against FM; anything the app is
   unsure about is flagged.
3. Go to the Training page, press **Preview** to see the plan, then **Apply training now** to write it into FM's
   calendar.
4. Changes only become permanent when you **save in FM**. Until then, every change can be undone in the app.
   Use *Back up saves* before you try something new.

## When something goes wrong

| What you see | What to do |
|---|---|
| *Not connected* | Update to the latest version first. Make sure a save is loaded. On a Mac: quit FM and use *Start FM with module*. On Windows: click *Reconnect*. |
| *No human manager with a club found* | You are on FM's start screen or unemployed in the save. Load a save in which you manage a club. If you do, update the app: this was the error after FM's November updates. |
| *Training calendar not found* | Update to 0.2.5 or later. |
| *This FM build is untested: allow changes in Settings first* | FM was patched and the app has not been checked on the new build yet. Reading still works. To make changes anyway: back up your save, then switch on *Allow changes on an untested FM build* in Settings. |
| *FM27 is already running* (Mac) | Save, quit FM completely, then click *Start FM with module* again. |
| The Breakdown is read wrong or only partly | Keep the whole Breakdown visible in FM's window and do not cover it with another window. On Windows with FM in Chinese, Japanese, Korean, Russian or Greek, add that language with *Optical character recognition* in Windows' language settings. |
| The Mac app will not open | Download the DMG again from the link above; versions 0.2.2 and 0.2.3 had no Mac download. |

## Reporting a problem

Please include:

- the app version (shown in the app) and whether you are on Windows or Mac;
- where FM comes from: Steam, Game Pass / Xbox app or Epic;
- the exact error text, or a screenshot of it;
- for a wrong Breakdown reading: `last-breakdown.png` and `last-analysis.txt` (Settings → *Open app folder*).

Report it in the thread where you found the app or as an [issue on GitHub](https://github.com/SemHiel2003/fm27-trainer/issues).
