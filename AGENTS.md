# Repository Guidelines

## Project Structure & Module Organization

OpenCapt is a Windows-only Rust 2024 desktop screenshot utility. Application startup and orchestration live in `src/main.rs` and `src/app.rs`. Capture, overlay, annotation, pin-window, tray, and hotkey behavior are split across top-level modules and `src/overlay/`. Configuration code is under `src/config/`, settings UI under `src/settings/`, and provider-specific OCR and translation code under `src/ocr/providers/` and `src/translation/providers/`. Keep protocol details in provider modules; the overlay should consume normalized results.

Icons are stored in `assets/icons/`. User and developer documentation is in `docs/`, with English mirrors in `docs/en/`. Treat `target/` and packaged output in `dist/` as generated artifacts.

## Build, Test, and Development Commands

Run commands from the repository root in PowerShell:

- `cargo run` — start the normal tray application.
- `cargo run -- capture-test` — exercise screen capture directly.
- `cargo run -- overlay-test` — open the screenshot overlay for interaction testing.
- `cargo run -- settings` — open settings without the tray workflow.
- `cargo test` — run the inline Rust unit tests.
- `cargo build` — compile a debug build.
- `cargo fmt --all -- --check` — verify standard Rust formatting.
- `cargo clippy --all-targets --all-features` — catch common Rust issues.
- `.\build-release.ps1` — create release artifacts; use `-StaticCRT` or `-SkipZip` when needed.

## Coding Style & Naming Conventions

Use `rustfmt` defaults (four-space indentation). Follow Rust conventions: `snake_case` for modules, functions, and tests; `PascalCase` for types and traits; `SCREAMING_SNAKE_CASE` for constants. Prefer clear module boundaries and small, focused changes over new abstractions. Keep comments sparse and update both Chinese and English documentation for user-visible behavior or configuration changes.

## Testing Guidelines

Place unit tests beside implementation code in `#[cfg(test)] mod tests`; name tests after the behavior being verified. No coverage threshold is enforced. Before submitting, run `cargo test` and `cargo build`. For capture, overlay, annotation, OCR, translation, or pin-window changes, manually verify first and repeated captures, cancel/save/copy, relevant tools, multiple monitors, and non-100% display scaling.

## Commit & Pull Request Guidelines

Recent history uses concise Conventional Commit prefixes such as `feat:`, `fix:`, `docs:`, `refactor:`, and `chore:`. Keep each commit and PR focused. PRs should include a summary, key changes, validation results, and linked issues when applicable. Attach screenshots or recordings for UI changes and note documentation or configuration updates.

## Security & Configuration

Do not commit API keys or local `config.toml`/`logs/` contents. Preserve the documented portable-path fallback behavior when changing configuration or output paths.
