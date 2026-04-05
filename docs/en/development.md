# Development and Debugging

中文：[../development.md](../development.md)

## Environment

- Windows 10 / 11
- Rust toolchain
- a terminal where `cargo` and `rustc` are directly available

If Cargo is not on `PATH`, use:

```powershell
$env:PATH="$env:USERPROFILE\.cargo\bin;$env:PATH"
```

## Common Startup Modes

Normal app startup:

```powershell
cargo run
```

Capture probe:

```powershell
cargo run -- capture-test
```

Open the screenshot overlay directly:

```powershell
cargo run -- overlay-test
```

Open the settings window directly:

```powershell
cargo run -- settings
```

## Config and Log Paths

OpenCapt prefers portable paths:

```text
.\config.toml
.\logs\
```

If the executable directory is not writable, it falls back to:

```text
%APPDATA%\OpenCapt\config.toml
%APPDATA%\OpenCapt\logs\
```

Screenshots are saved by default to:

```text
%USERPROFILE%\Pictures\OpenCapt\
```

## Packaging

```powershell
.\build-release.ps1
```

Useful options:

```powershell
.\build-release.ps1 -StaticCRT
.\build-release.ps1 -SkipZip
```

## Debugging Tips

### Changing tray / hotkey / config hot reload

Look at:

- `src/app.rs`
- `src/tray.rs`
- `src/hotkey.rs`
- `src/config/*`

### Changing capture interaction / selection / annotation

Look at:

- `src/overlay/state.rs`
- `src/overlay/input.rs`
- `src/overlay/render.rs`
- `src/overlay/draw.rs`

### Changing text tools

Look at:

- `src/overlay/text.rs`
- `src/overlay/render.rs`

### Changing OCR / translation providers

Look at:

- `src/ocr/*`
- `src/translation/*`

## Common Pitfalls

### 1. Do not merge settings-window concerns into the capture flow

The settings window is a separate subsystem. The overlay is optimized for native screenshot interaction, not for general GUI patterns.

### 2. Do not put provider protocol details directly into overlay

Protocol-specific logic belongs in provider modules. Overlay should consume normalized results.

### 3. Always think about high DPI and multi-monitor behavior

Screenshot tools often break around:

- coordinate mismatch
- scaling mismatch
- first-capture cache behavior

After touching the capture flow, at minimum manually test:

- non-100% scaling
- multi-monitor setups
- first capture and repeated captures

### 4. Pin windows and overlay are different native windows

Both use layered windows, but they are different subsystems. Do not assume a rendering fix in one will automatically apply to the other.

## Recommended Local Validation

Run at least:

```powershell
cargo test
cargo build
```

If you changed the capture flow, also manually verify:

- hotkey capture
- selection, cancel, save, copy
- annotation editing
- OCR / translation
- pin windows