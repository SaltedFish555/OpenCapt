# Contributing

中文：[CONTRIBUTING.md](CONTRIBUTING.md)

Thanks for your interest in OpenCapt.

This guide only defines the core collaboration expectations. The goal is to keep issues and pull requests clear and low-friction, not to add heavyweight process.

## Before Opening an Issue

Please try to confirm:

- you are using the latest code or latest release
- the behavior is not already explained in the README or `docs/`
- there is no existing issue for the same problem

For bug reports, include as much of the following as possible:

- Windows version
- display scaling and monitor count
- reproduction steps
- expected behavior vs actual behavior
- logs, screenshots, or screen recordings

## Before Opening a Pull Request

Please keep these conventions in mind:

- keep changes focused; one PR should solve one kind of problem
- avoid mixing unrelated refactors, formatting churn, or broad renames
- include screenshots or notes if UI/interaction changed
- describe how the change was validated

## Local Validation

At minimum, run:

```powershell
cargo test
cargo build
```

If your change touches capture, annotation, pin windows, OCR, or translation, also include manual verification notes, for example:

- hotkey capture
- cancel / save / copy
- annotation tools
- pin windows
- OCR / translation
- high-DPI or multi-monitor scenarios

## Code Style Expectations

OpenCapt currently favors these principles:

- runtime stability is more important than abstraction for its own sake
- extend existing module boundaries where possible instead of pushing protocol details into overlay
- keep the Windows-only assumption unless a broader change is explicitly planned
- use comments sparingly; prefer clear structure and supporting docs

## Keep Documentation in Sync

These changes usually require doc updates:

- user-visible features
- config fields or defaults
- new OCR / translation providers
- changes to startup modes, build flow, or path behavior

At minimum, check:

- [README.md](README.md)
- [README.en.md](README.en.md)
- [docs/](docs)

## Communication

If the change is large, opening an issue first is recommended.  
For smaller fixes or doc updates, opening a PR directly is fine.

