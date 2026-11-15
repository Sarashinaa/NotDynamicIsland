# NotDynamicIsland - Project Guidelines & Rules

## 1. Core Performance & Resource Targets
- **Binary Size**: Strictly under 1.0 MB (< 1024 KB) for the release binary.
- **CPU Usage**: 0.0% CPU when idle (no polling loops, event-driven Win32 hooks & message pumps only).
- **RAM Usage**: peak/max at 10MB. lower is better. 
- **Dependencies**: Zero heavyweight GUI crates (no Electron, no Tauri, no Slint, no winit). Pure Win32 API + Direct2D / manual pixel buffer software rendering.

## 2. Process Execution Rule (CRITICAL)
- **DO NOT auto-run or launch `not_dynamic_island.exe`** after compiling/building or fixing issues.
- When built via background process, it may run headless or detached and not appear visually on screen.
- Always notify the user that the build is finished and let the user launch or restart the application manually.

## 3. Git & Security Rules
- **DO NOT push to GitHub** (`git push`) unless explicitly requested and approved by the user.
- **Repository Cleanliness**:
  - Keep the repository clean of runtime logs (`*.log`), test output bitmaps (`*.bmp`), and temp files.
  - Never commit API keys, personal credentials, or local environment paths into git.
  - Groq API keys must reside in `%USERPROFILE%\.groq\api_key` outside the repository and remain gitignored (`*groq*`).
  - Personal calendar sync links, todo tasks, and tokens must remain in user data paths or `.gitignore`.

## 4. Architecture & Component Standards
- **Text Inputs**: All text inputs (AI Assistant prompt, To-Do task creation, Calendar event input) must use the unified `TextInput` engine with native Windows Word/Notepad shortcuts (Arrow keys, Shift selection, Home/End, Ctrl+A select all, Ctrl+C copy, Ctrl+V paste, Ctrl+X cut, Delete key, and sub-pixel hit-test click positioning).
- **Glassmorphism**: Apple Liquid Glass / Obsidian dark theme with SDF rounded corners, subtle diffusion glow, and smooth kinetic spring physics.
