# NotDynamicIsland 🏝️

A lightweight, native Dynamic Island desktop overlay for Windows 11 and 10, written from scratch in **pure Rust**.

No Electron. No Chromium. No heavy UI runtimes. Just clean Win32 APIs, software-rendered Signed Distance Fields (SDF), and smooth spring physics.

---

## ✨ Features

### 🎵 Media Controller & Synced Lyrics
- **Universal Media Controls**: Integrates with Windows System Media Transport Controls (GSMTC) — works out of the box with Spotify, Apple Music, YouTube Music, Chrome, and local players.
- **Real-Time Synced Lyrics**: Fetches synchronized LRC lyrics automatically with smooth, kinetic auto-scrolling.
- **Romaji & Romanization Toggle ("RO")**:
  - Automatically romanizes **Japanese (Hiragana, Katakana, Kanji)** and **Korean (Hangul)** lyrics.
  - Zero-lag Korean mathematical Unicode syllable decomposition.
  - Seamless toggle button to switch between original script and Romaji in 1:1 synchronization.
  - 100% monochrome Apple glass badge styling.

### 💎 Dark Liquid Glass Design
- **Smoked Obsidian Material**: Rich, uniform dark glass without artificial washed-out vertical gradients or cloudy gray bands.
- **Refractive Bevel Lighting**: Physics-based edge lighting and specular cut-glass rim highlights that trace the notch contour.
- **Dual Atmospheric Shadows**: Layered contact and ambient drop shadows that let the island float naturally over any wallpaper.
- **Click-Through Passthrough**: Transparent areas outside the notch pass clicks through directly (`HTTRANSPARENT`), so you can click windows behind it without interference.

### 📋 Productivity & Tools
- **File Shelf**: Drag and drop any file onto the island for quick temporary storage. Click to reveal in File Explorer.
- **To-Do List**: Inline task manager with checkboxes, task count stats, smooth mouse wheel scrolling, and navigation arrows.
- **Calendar & Agenda**: Interactive monthly calendar grid with daily event agenda.
  - **Google Calendar Sync**: Native background synchronization via secret `.ics` feed URL.
  - **Local Timezone Offset**: Automatic UTC-to-local conversion.
- **Focus Timer**: Configurable Pomodoro focus timer and stopwatch with one-click presets.

### 🔔 System HUD & Notifications
- **Volume OSD**: Event-driven volume slider via Windows Core Audio (WASAPI).
- **Power & Battery**: Live battery ring indicator, charging pulse, and battery percentage HUD.
- **Display Brightness**: Smooth brightness percentage OSD.
- **Privacy Indicators**: Green and orange dots when microphone or camera are accessed by any app.
- **Screen Snip & Color Picker**: Quick-action system utilities integrated into the island tools menu.

---

## ⚡ Performance

| Metric | Measured |
| :--- | :--- |
| **RAM Usage** | **~3 – 8 MB** (resets to ~1.9 MB on idle) |
| **CPU Usage (Idle)** | **0.0%** (event-driven message loop, sleeps completely when not animating) |
| **Binary Size** | **~860 KB** (single standalone executable) |
| **GPU VRAM** | **0 MB** (CPU software rasterizer with per-pixel alpha blending) |
| **Startup Time** | **< 50 ms** instant launch |

---

## 🚀 Getting Started

### Pre-built Binary
1. Download `not_dynamic_island.exe` from the [Releases](https://github.com/Sarashinaa/NotDynamicIsland/releases/latest) page.
2. Double-click `not_dynamic_island.exe` to run.
3. The island will anchor itself to the top-center of your primary display.

> **Tip (Auto-start on Boot):**  
> Press `Win + R`, type `shell:startup`, and place a shortcut to `not_dynamic_island.exe` inside that folder.

---

## 🎮 Navigation & Controls

| Action | Control |
| :--- | :--- |
| **Expand / Collapse** | Left-click on the notch |
| **Switch Pages** | Click `<` / `>` buttons, or click the bottom page indicator dots |
| **Scroll (Lyrics / To-Do)** | Scroll mouse wheel over the expanded pane |
| **Toggle Lyrics** | Click the speech bubble icon `💬` in the music controller |
| **Toggle Romaji** | Click the `RO` badge beside the lyrics button |
| **Google Calendar Sync** | Paste your Google Calendar secret iCal URL into `calendar_sync.txt` |
| **Drag & Drop Shelf** | Drag any file from File Explorer and drop it onto the island |

---

## 🛠️ Building from Source

### Prerequisites
- Windows 10 or 11
- [Rust toolchain](https://rustup.rs/) (stable, 2021 edition)

### Build Commands
```powershell
# Clone the repository
git clone https://github.com/Sarashinaa/NotDynamicIsland.git
cd NotDynamicIsland

# Run tests
cargo test

# Build optimized release binary
cargo build --release
```

The compiled standalone executable will be located at:
`target\release\not_dynamic_island.exe`

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
