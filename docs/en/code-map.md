# OpenCapt Code Map

中文：[../code-map.md](../code-map.md)

## Where to Start

If this is your first time reading the repository, do not jump straight into overlay internals. A better order is:

1. `src/main.rs`
2. `src/app.rs`
3. `src/config.rs` and `src/config/*`
4. `src/overlay.rs` and `src/overlay/*`
5. `src/settings.rs` and `src/settings/*`
6. `src/ocr/*` and `src/translation/*`
7. `src/pin.rs`

This gives you the runtime model first, then the screenshot and editing details.

## Entry Layer

### `src/main.rs`

Owns:

- startup-mode parsing
- config and path loading
- logging initialization
- dispatch into the main runtime, settings window, or debug helpers

Start here when you need to understand why the program enters a specific mode.

### `src/app.rs`

Owns:

- the main event loop
- tray, hotkey, overlay, pin, and settings coordination
- config hot reload

This is the runtime control center. If the issue involves tray actions, hotkeys, or config changes, look here first.

## Capture and Annotation Layer

### `src/overlay.rs`

This is the overlay façade. It:

- exports the public overlay types
- wires child modules together
- defines top-level constants and high-level structures

### `src/overlay/state.rs`

Owns internal state such as:

- current tool
- current selection
- annotation object collections
- OCR / translation overlay state
- drag, resize, and text-edit modes

### `src/overlay/input.rs`

Owns input handling:

- mouse events
- keyboard events
- toolbar actions

If you are changing interaction behavior, this is usually the first file to inspect.

### `src/overlay/render.rs`

Owns rendering and export:

- selection and dimmed background
- annotation object drawing
- OCR / translation overlay rendering
- final exported image generation

### `src/overlay/draw.rs`

Owns low-level drawing primitives:

- pixel drawing
- GDI text rasterization
- `tiny-skia` anti-aliased paths
- SVG icon blitting

If visuals are wrong but interaction logic is correct, the issue is often here.

### `src/overlay/text.rs`

Owns text-tool specific logic:

- text box layout
- caret and editing state
- text style behavior

### `src/overlay/toolbar.rs`

Owns toolbar layout and button definitions.

### `src/overlay/win32.rs`

Owns the native overlay window pieces:

- window class registration
- layered surface
- `wndproc`
- cursor switching

## Settings and Config Layer

### `src/config.rs` + `src/config/*`

Split by responsibility:

- `types.rs`: config types and defaults
- `compat.rs`: backward compatibility
- `paths.rs`: portable / appdata path selection
- `io.rs`: load and save

If you add a config field, it will almost always involve this subsystem.

### `src/settings.rs` + `src/settings/*`

Split into support modules and page modules:

- `theme.rs`: visual theme
- `profiles.rs`: model-related labels and options
- `pages/*`: rendering logic for each settings page

For settings issues, look here before touching `app`.

## Provider Layer

### `src/ocr/*`

- `mod.rs`: unified entry point
- `parse.rs`: provider response parsing
- `normalize.rs`: bbox normalization
- `providers/*`: provider-specific OCR protocol adapters

### `src/translation/*`

- `mod.rs`: unified entry point
- `parallel.rs`: block-level parallel translation scheduling
- `parse.rs`: translation response parsing
- `providers/*`: provider-specific translation adapters

## Other Support Modules

- `src/capture.rs`: screen capture and UI element probing
- `src/output.rs`: clipboard copy and PNG save
- `src/pin.rs`: pinned image windows
- `src/startup.rs`: launch-at-startup synchronization
- `src/tray.rs`: tray and menu
- `src/hotkey.rs`: global hotkey registration
- `src/memory.rs`: working-set trimming after capture