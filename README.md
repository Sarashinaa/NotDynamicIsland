# Project Nova (NotDynamicIsland) 🏝️

Sistem hamparan desktop (*Dynamic Island*) berkinerja ekstrem untuk Windows 11 yang dibangun **100% dari nol** menggunakan bahasa pemrograman **Rust asli (Native)**. 

Proyek ini dibuat untuk memberikan pengalaman antarmuka yang sangat mulus dan estetis tanpa kompromi keamanan: **bebas modifikasi pihak ketiga yang tidak jelas, nirkoneksi luar, 100% kode terbuka (open-source), dan imun dari ancaman infostealer/malware**.

---

## 📥 Unduh Langsung (Untuk Teman / Pengguna Awam)

Tidak perlu paham koding, tidak perlu install Rust, dan tidak perlu buka PowerShell/terminal sama sekali!

1. Buka halaman rilis terbaru: **[Download NotDynamicIsland (Releases)](https://github.com/Sarashinaa/NotDynamicIsland/releases/latest)**
2. Unduh file **`NotDynamicIsland-windows-x64.zip`** (atau langsung file **`not_dynamic_island.exe`**).
3. Ekstrak file zip, lalu **double-click `not_dynamic_island.exe`**.
4. Dynamic Island langsung aktif di layar atas monitor Anda!

> **Tips:** Mau aplikasi langsung jalan otomatis saat laptop/PC dinyalakan?
> Cukup buat shortcut dari `not_dynamic_island.exe`, tekan tombol `Win + R`, ketik `shell:startup`, lalu paste shortcut-nya di sana!

---

## 📊 Hasil Pengukuran & Benchmark Aktual

Sesuai dengan target ketat pada [rancangan_arsitektur_dynamic_island.md](file:///d:/PROJECT%20ISENG/NotDynamicIsland/rancangan_arsitektur_dynamic_island.md) dan [prd_dynamic_island.html](file:///d:/PROJECT%20ISENG/NotDynamicIsland/prd_dynamic_island.html):

| Parameter | Target PRD | Hasil Implementasi Aktual |
| :--- | :--- | :--- |
| **Konsumsi RAM (Working Set)** | < 30 MB | **~1.92 MB** *(15x lebih hemat dari target!)* |
| **Penggunaan CPU saat Diam (Idle)** | 0% | **0.0%** *(Thread tidur pulas dalam `GetMessageW`)* |
| **Ukuran Biner Eksekusi** | Ringan | **459 KB** *(Single standalone .exe)* |
| **Driver GPU Eksternal** | Nirketergantungan | **0 MB GPU VRAM** *(CPU Software Renderer)* |
| **Arsitektur Pemantauan** | Event-Driven | **100% Berbasis Peristiwa (Zero Polling)** |

---

## 🏛️ Fondasi Arsitektur (The 4 Pillars & EcoQoS)

### 1. Pillar 1: Native Code & Memory < 30 MB
- Menggunakan bahasa pemrograman **Rust** tanpa mesin web browser (Electron / Tauri dieliminasi total).
- Mengintegrasikan **mimalloc** (alokator berkinerja tinggi dari Microsoft Research) sebagai `#[global_allocator]` untuk memberantas fragmentasi memori.
- **CPU Software Renderer**: Rasterisasi langsung ke penyangga 32-bit BGRA pre-multiplied dengan *Desktop Window Manager (DWM)* `UpdateLayeredWindow`. Menghilangkan beban inisialisasi driver GPU DirectX/Vulkan yang memakan 70–150 MB.

### 2. Pillar 2: Draws Only When Moving (0% Idle CPU)
- **Model Fisika Pegas (Mass-Spring-Damper)**: Seluruh transisi geometri (lebar, tinggi, sudut lengkung, opasitas) disimulasikan menggunakan dinamika pegas kinetik dengan kurva pantulan elegan (`spring(bounce)`).
- **Penghentian Siklus Cerdas**: Begitu elemen mencapai posisi diam presisi, loop perenderan berhenti seketika (`KillTimer`), membiarkan utas tertidur lelap tanpa memakan siklus CPU.

### 3. Pillar 3: Waits Instead of Checking (Zero Polling)
- **Media Controller**: Terkoneksi ke Windows Media Transport (*GSMTC*) via WinRT. Bereaksi seketika saat lagu berganti atau tombol play/pause ditekan pada Spotify, YouTube, Chrome, dll.
- **Volume OSD**: Mengimplementasikan antarmuka COM `IAudioEndpointVolumeCallback` via Windows Core Audio API (*WASAPI*). Menerima notifikasi kernel saat volume diubah.
- **Power & Battery OSD**: Mendaftarkan `RegisterPowerSettingNotification` dengan `GUID_ACDC_POWER_SOURCE` untuk menerima `WM_POWERBROADCAST` saat charger laptop dicolok atau dicabut.
- **Caps Lock OSD**: Mendengarkan perubahan status tombol Caps Lock secara event-driven.

### 4. Pillar 4: Zero-Copy File Shelf
- **Drag & Drop**: Mengadopsi filosofi pointer-oriented murni. Saat berkas diseret dan dilepas ke hamparan, sistem hanya menyimpan referensi `PathBuf` string (tanpa memuat isi berkas ke RAM).
- Ekstraksi ikon sistem operasi menggunakan `SHGetFileInfoW` (`SHGFI_ICON | SHGFI_LARGEICON`).
- **Destruksi Ikon Ketat**: Memanggil `DestroyIcon` seketika untuk mencegah kebocoran *GDI Handles*.

### 5. EcoQoS & Working Set Trimming
- **Efficiency Core Pinning**: Memanggil `SetProcessInformation` dengan `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` & `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION` agar Windows menjadwalkan proses hanya pada E-Cores dan mengizinkan CPU memasuki Deep C-States.
- **Idle Working Set Trimming**: Saat modul kembali ke titik siaga (*idle capsule*), memanggil `SetProcessWorkingSetSizeEx` untuk menyita halaman memori tak terpakai, mereset residu RAM menjadi hanya ~1.9 MB.

---

## 🪟 Struktur Proyek

```
d:\PROJECT ISENG\NotDynamicIsland\
├── Cargo.toml               # Dependensi (windows 0.58, mimalloc, fontdue) & profil rilis LTO
├── rancangan_arsitektur_dynamic_island.md
├── prd_dynamic_island.html
├── src\
│   ├── main.rs              # Entry point, event loop, message dispatch, global mimalloc
│   ├── physics\
│   │   ├── mod.rs
│   │   └── spring.rs        # Simulasi fisika kinetik Mass-Spring-Damper
│   ├── render\
│   │   ├── mod.rs
│   │   ├── buffer.rs        # Penyangga piksel 32-bit BGRA CPU dengan per-pixel alpha blending
│   │   ├── primitives.rs    # SDF anti-aliased squircle, shadow lembut, progress bar
│   │   ├── text.rs          # Tipografi Segoe UI sub-pixel rasterizer (fontdue)
│   │   └── icons.rs         # Ikon vektor prosedural tajam (Play, Pause, Skip, Vol, Bat, Shelf, Close)
│   ├── system\
│   │   ├── mod.rs
│   │   ├── audio.rs         # WASAPI Core Audio volume callback
│   │   ├── media.rs         # WinRT GSMTC listener & interactive controller
│   │   ├── power.rs         # Power broadcast notifier (AC/DC charger)
│   │   ├── keyboard.rs      # Low-level hook untuk status Caps Lock
│   │   ├── shelf.rs         # Zero-copy file shelf dengan SHGetFileInfoW & DestroyIcon
│   │   └── eco_qos.rs       # EcoQoS execution speed throttling & memory trim
│   ├── ui\
│   │   ├── mod.rs
│   │   ├── state.rs         # State machine pulau (Idle, Media, Volume, Power, FileShelf, CapsLock)
│   │   └── layout.rs        # Perhitungan tata letak geometris & hit-testing tombol interaktif
│   └── window\
│       ├── mod.rs
│       └── win32.rs         # TopMost Layered ToolWindow, DIBSection, penembusan klik WM_NCHITTEST
```

---

## 🎮 Interaksi Pengguna & Cara Kerja

1. **Idle Capsule**:
   - Menempati bagian atas tengah layar sebagai kapsul hitam obsidian elegan dengan bayangan lembut.
   - Menampilkan titik status aksen biru dan jam sistem.
   - Jika musik sedang diputar, menampilkan animasi bar equalizer dan judul lagu secara ringkas.
   - **Hit-Testing Passthrough**: Seluruh area transparan di luar kapsul menembuskan klik tetikus (`HTTRANSPARENT`), sehingga aplikasi di belakangnya (browser, game, kode) tetap dapat diklik tanpa gangguan!
2. **Media Controller**:
   - Klik pada kapsul saat memutar musik untuk memperluas (*expand*) pulau.
   - Menampilkan thumbnail equalizer, judul lagu tebal, nama artis, serta tombol **Previous**, **Play/Pause**, dan **Next**.
   - Klik tombol untuk mengontrol pemutar musik Anda secara langsung.
3. **Volume Pop-up**:
   - Saat Anda mengubah volume sistem (lewat tombol keyboard / fn), pulau melesat membesar menampilkan ikon speaker, progress bar dinamis, dan persentase volume, lalu kembali ke bentuk kapsul secara otomatis.
4. **Power & Battery Pop-up**:
   - Saat charger dicolok atau dicabut, pulau memantul menampilkan status pengisian daya dan sisa baterai.
5. **Caps Lock Pop-up**:
   - Menampilkan lencana status saat Caps Lock ditekan.
6. **File Shelf (Seret & Lepas)**:
   - Seret berkas apa saja dari Windows Explorer dan jatuhkan ke area pulau.
   - Pulau akan memperluas diri menjadi rak berkas sementara (*File Shelf*), menampilkan ikon asli Windows untuk jenis berkas tersebut, nama berkas, dan ukurannya.
   - Klik berkas untuk membukanya langsung di Explorer, atau klik tombol silang (X) untuk mengosongkannya.

---

## 🚀 Cara Menjalankan & Membangun

### Menjalankan Versi Rilis Cepat
Buka PowerShell di folder proyek ini:
```powershell
cargo run --release
```

### Membangun Biner Standalone (.exe)
```powershell
cargo build --release
```
Biner mandiri yang sangat kecil (~459 KB) akan dihasilkan di:
`target\release\not_dynamic_island.exe`

Anda dapat menyalin file `not_dynamic_island.exe` ini ke mana saja, membuat shortcut ke folder Startup Windows (`shell:startup`), atau menjalankannya secara portabel tanpa memerlukan runtime atau instalasi tambahan!
