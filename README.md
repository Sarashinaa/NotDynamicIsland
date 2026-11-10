# NotDynamicIsland 🏝️

A native, ultra-lightweight Dynamic Island companion for Windows 10 & 11, written from scratch in pure Rust.

No Electron, no Chromium, and no heavy runtime dependencies. It uses direct Win32 layered window APIs, an in-house software Signed Distance Field (SDF) rasterizer, and mass-spring-damper kinetics to deliver a fluid desktop experience with near-zero resource consumption.

---

## What's New in v1.1

- **Liquid Glass Materials**: Choose between **Dark Obsidian** (deep, translucent tinted glass) and **Crystal-Clear Light** (high-clarity translucent glass). Features dual-stroke optical beveling (1px gradient outer edge + 1.5px prism refraction) and a 1.8px wet-look specular light arc on the top notch curve.
- **Kinetic Sun / Moon Toggle**: Apple-style sliding spring toggle with critically damped physics (`ζ ≈ 0.717`). Clicking the switch glides the thumb capsule and smoothly morphs the notch glass between themes in real time.
- **Synced Lyrics & Navigation**:
  - Borderless, clean scrolling lyrics with Apple Music-style edge fades.
  - **Romaji Toggle (`RO`)**: 1-click romanization for Japanese (Kanji / Kana) and Korean (Hangul) songs.
  - Instant ±5 second seek scrub buttons and interactive dragging slider.
- **To-Do Navigation**: Added dedicated right navigation arrow (`>`) for seamless cycling across all pages.
- **High-Contrast Monochrome HIG**: Crisp white typography and icons across all modes for optimal readability over dark and light desktop wallpapers.

---

## Features

### 🎵 Media Player & Synchronized Lyrics
- **Universal Media Controls**: Reads playback state via Windows System Media Transport Controls (GSMTC) — works out of the box with Spotify, Apple Music, YouTube Music, web browsers, and local players.
- **Live Synced Lyrics**: Fetches synchronized LRC lyrics automatically with smooth auto-scroll.
- **Romanization Engine**: On-the-fly Japanese phonetic transliteration and mathematical Unicode Hangul syllable decomposition.
- **Scrubbing & Controls**: Interactive progress bar with circular thumb knob, play/pause, track skipping, and ±5-second jump buttons.

### 💎 Liquid Glass Design
- **Two Optical Themes**: Dark Obsidian and Crystal-Clear Light, selectable at runtime with saved preference.
- **Physics-Based Refraction**: Clean internal edge refraction and top specular reflections without artificial gray banding or milky haze.
- **Seamless Desktop Passthrough**: Precision hit-testing (`HTTRANSPARENT`) outside the curved notch allows clicking and dragging windows directly behind the canvas.

### 🛠️ Desktop Utilities
- **Drop Shelf**: Drag and drop files onto the island for quick temporary staging. Click to reveal in File Explorer.
- **To-Do List**: Inline task list with completion checkboxes, progress counter, mouse wheel scrolling, and navigation arrows.
- **Calendar & Agenda**: Monthly calendar view with daily agenda. Supports private Google Calendar background sync via secret iCal URL (`calendar_sync.txt`).
- **Focus Timer**: Pomodoro focus timer and stopwatch with one-click presets.

### 🔔 Native System HUDs
- **Volume OSD**: Event-driven volume HUD hooked into Windows Core Audio (WASAPI).
- **Battery & Power**: Circular battery ring gauge, charging status, and battery saver indicator.
- **Display Brightness**: Smooth brightness HUD.
- **Privacy Indicators**: Real-time microphone and camera access indicator dots.

---

## Performance

| Metric | Measured |
| :--- | :--- |
| **RAM Usage** | **~3 – 8 MB** active (trimmed to **~1.8 MB** at idle) |
| **CPU Usage (Idle)** | **0.0%** (blocks on `GetMessageW`, zero polling) |
| **Executable Size** | **~844 KB** (single standalone `.exe`, zero install required) |
| **GPU VRAM** | **0 MB** (efficient CPU software rasterizer with per-pixel alpha blending) |
| **Cold Startup** | **< 50 ms** |

---

## Getting Started

### Download
1. Grab the latest standalone `not_dynamic_island.exe` from [GitHub Releases](https://github.com/Sarashinaa/NotDynamicIsland/releases/latest).
2. Run `not_dynamic_island.exe`. It automatically anchors to the top center of your primary display.
3. To start automatically on boot: press `Win + R`, type `shell:startup`, and place a shortcut to the executable there.

---

## Controls

| Action | Control |
| :--- | :--- |
| **Expand / Collapse** | Left-click the notch |
| **Toggle Light / Dark Mode** | Click the Sun / Moon toggle at the top-left of the Music player |
| **Cycle Pages** | Click `<` / `>` arrows, or click the bottom page indicator dots |
| **Open Lyrics** | Click the speech bubble icon `💬` in the Music player |
| **Toggle Romaji** | Click the `RO` badge beside the lyrics button |
| **Seek ±5s** | Click `↺ 5` or `↻ 5` buttons |
| **Scroll Lists** | Scroll mouse wheel over lyrics, to-do list, or shelf |
| **Temporary File Staging** | Drag any file from Windows Explorer onto the island |

---

## Building from Source

### Requirements
- Windows 10 or 11
- [Rust toolchain](https://rustup.rs/) (stable channel, edition 2024 / 2021)

```powershell
# Clone the repository
git clone https://github.com/Sarashinaa/NotDynamicIsland.git
cd NotDynamicIsland

# Run the test suite
cargo test

# Compile release binary
cargo build --release
```

The resulting standalone executable will be at `target\release\not_dynamic_island.exe`.

---

## License

This project is licensed under the [MIT License](LICENSE).
