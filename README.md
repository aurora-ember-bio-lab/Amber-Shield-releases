# Amber Shield Lite

Author: Aurora Ember Bio Lab
Copyright: 2026 Aurora Ember Bio Lab
License: MIT (see `LICENSE`)

This is the free, open-source edition of Amber Shield: a Rust + Tauri
local-first security scanner. It's a deliberate subset of the full
(paid) Amber Shield product - see "What's in this edition" below for
exactly what's here and what isn't, and why.

Every module here compiles and its unit tests pass (`cargo test
--workspace`); this is real, working code, not a mockup.

## What's in this edition

Included (free, MIT):
- **Code-vulnerability heatmap** (`codemap` + `security`): Trivy +
  GitLeaks + tree-sitter based file heat-scoring.
- **Local AI explain-drawer** (`llm`): a local-only Ollama client that
  explains HUD events. No cloud fallback, no API key, costs nothing to
  run.
- **Event log/storage** (`storage`, `logwatch`): the hot/cold event
  store and the cross-platform log/event interceptor (including a local
  Stripe CLI event source, if you have `stripe` on `PATH`).
- **Optional license check** (`hwid`): present for forward
  compatibility, but this build runs fully with no license installed.

Not included - these are paid-tier features that live only in the
private Amber Shield source tree, not in this repo:
- The process-behavior monitor (living-off-the-land / suspicious-spawn
  detection).
- The Postgres database query monitor.
- Stripe subscription checkout/billing and the in-app payment widget.
- The license-issuing server tool (private-key signing never belongs in
  a public repo regardless of tier).

If you want those, they're part of the paid Amber Shield product at
https://www.ambershield.app.

## Layout

```
amber-shield-lite/
  crates/
    core-engine/        framework-agnostic core - no Tauri types leak in here
      src/
        types.rs         the one Event shape every module emits
        hwid.rs           hardware fingerprint + Ed25519 license verification (optional)
        storage/          hot (sled) + cold (sqlite, duckdb optional) stores
        logwatch/         cross-platform log interceptor (per-OS adapters + Stripe CLI)
        security/         Trivy + GitLeaks process wrappers
        codemap/          tree-sitter based file heat-scoring (the heatmap wedge)
        llm/              local Ollama client for the AI explain-drawer
    tauri-app/           thin Tauri shell: commands + event forwarding, zero business logic
  frontend/              static HTML/CSS/vanilla JS - Canvas2D HUD
```

## Building

```sh
# type-check / compile everything
cargo check --workspace
cargo test --workspace

# full debug build, including linking against GTK/WebKit on Linux
cargo build -p amber-shield-lite

# native Linux bundles (AppImage and Debian package)
cargo tauri build --bundles appimage,deb
```

On Linux you'll need the Tauri system dependencies installed first:
`libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev
libayatana-appindicator3-dev librsvg2-dev` (apt package names; see
[Tauri's prerequisites doc](https://tauri.app/start/prerequisites/) for
macOS/Windows).

If you need to regenerate the full icon set from `icons/icon.png`:

```sh
cargo tauri icon crates/tauri-app/icons/icon.png
```

After a Linux bundle build, artifacts are written under
`target/release/bundle/`.

For the Windows build and release-tagging process, see `RELEASE_README.md`
in the private source repo - this repo is source-only, not the release
pipeline.

## License

MIT - see `LICENSE`. Third-party tool licenses (Trivy, GitLeaks, and the
Rust dependency tree) are in `THIRD_PARTY_NOTICES.md`.
