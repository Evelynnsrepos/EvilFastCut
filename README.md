<h1 align="center">EVIL FAST CUT</h1>

<p align="center">
  <b>Cuts the silences (and breaths) out of your videos for <a href="https://github.com/storytold/filmcraft">FilmCraft</a>, the open-source video editor.</b>
</p>

<p align="center">
  <a href="https://discord.gg/AGfaBwNKfN"><img src="https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://www.youtube.com/@EvelopmentGames"><img src="https://img.shields.io/badge/YouTube-Eve.Lopment%20Games-FF0000?logo=youtube&logoColor=white" alt="YouTube"></a>
  <a href="https://www.instagram.com/evelopmentgames/"><img src="https://img.shields.io/badge/Instagram-evelopmentgames-E4405F?logo=instagram&logoColor=white" alt="Instagram"></a>
  <a href="http://onee.page/evelopmentgames"><img src="https://img.shields.io/badge/Website-onee.page-8A2BE2" alt="Website"></a>
  <img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0">
</p>

<p align="center">
  <a href="https://ko-fi.com/evelynnevlmg"><img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Support me on Ko-fi"></a>
</p>
## What it does

Evil Fast Cut listens to your video, finds the parts with no sound, and gives you a FilmCraft
sequence with those parts already cut out. Turn on `--breaths` and it also cuts the breaths
between sentences. Nothing is changed in your original file.

You get:

- **`MyVideo-fastcut.xml`**: open it in FilmCraft with **File ▸ Import** and you get a new
  sequence, *MyVideo (fast cut)*, with every kept piece on the timeline, video and sound linked.
  You can still drag any cut open again if it took too much. (The same file also opens in
  Premiere Pro and DaVinci Resolve.)
- **A trimmed video** too, if you add `--video MyVideo-cut.mp4`.

FilmCraft doesn't load plug-ins yet, so Evil Fast Cut is a small program you run next to it.

## Install

1. Install **ffmpeg** (it reads the sound): [ffmpeg.org/download.html](https://ffmpeg.org/download.html).
   Windows: `winget install ffmpeg` · Mac: `brew install ffmpeg` · Linux: `sudo apt install ffmpeg`.
2. Download Evil Fast Cut for your computer from the
   [**Releases** page](https://github.com/Evelynnsrepos/EvilFastCut/releases/latest)
   (`evil-fastcut-windows.zip`, `-macos.zip` or `-linux.zip`) and unzip it.

## Use it

Open a terminal in the folder where you unzipped it and run:

```sh
evil-fastcut "C:\Videos\MyVideo.mp4"            # Windows (or drag the video onto the window)
./evil-fastcut ~/Videos/MyVideo.mp4             # Mac / Linux
./evil-fastcut MyVideo.mp4 --breaths            # also cut breaths
./evil-fastcut MyVideo.mp4 --video MyVideo-cut.mp4   # also save a trimmed video
```

Then in FilmCraft: **File ▸ Import**, pick `MyVideo-fastcut.xml`.

### Settings

| Option | Default | What it does |
|---|---|---|
| `--threshold DB` | `-40` | Anything quieter than this is silence. Noisy room or hum: try `-35`. Very quiet voice: try `-50`. |
| `--min-silence SEC` | `0.5` | Only cut silences at least this long. Lower it (`0.3`) for a snappier, jump-cut style. |
| `--padding SEC` | `0.1` | Sound kept before and after each cut so words aren't clipped. |
| `--breaths` | off | Also cut breaths: quiet sounds with no voice in them, 0.15-1.2 s long, between phrases. |
| `--breath-drop DB` | `12` | How much quieter than your speech a breath is. Lower it (`8`) if loud breaths stay in. |
| `--xml FILE` | next to the video | Where to save the sequence. |
| `--video FILE` | off | Also render a trimmed video with ffmpeg. |

Run `evil-fastcut --help` to see them all.

**Tips:** if words get cut off, raise `--padding` or lower `--threshold`. Breath detection is a
best guess: an "s" or "f" at the end of a word that trails off can look like a breath, so check
the cuts and drag them open in FilmCraft if needed.

## Build from source (for developers)

```sh
cargo build --release   # → target/release/evil-fastcut
cargo test
```

Releases are built for Windows, macOS and Linux by `.github/workflows/release.yml` when a `v*`
tag is pushed, or from **Actions ▸ Release ▸ Run workflow**.

## Community

- Discord: https://discord.gg/AGfaBwNKfN
- YouTube: https://www.youtube.com/@EvelopmentGames
- Instagram: https://www.instagram.com/evelopmentgames/
- Like it? You can buy me a coffee on [Ko-fi](https://ko-fi.com/evelynnevlmg).
- Ideas and bugs: [open an issue](https://github.com/Evelynnsrepos/EvilFastCut/issues/new)

## License

MIT OR Apache-2.0, at your option.
