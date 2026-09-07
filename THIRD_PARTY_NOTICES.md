# Third-party notices

Amber Shield Lite calls the following tools as external processes (`std::process::Command`,
see `crates/core-engine/src/security/`). They are **not vendored/bundled** in this
repo - the code shells out to whatever copy is on `PATH` (or at an
`ATLAS_*_BIN` override). If a future build bundles the actual binaries, copy
each project's `LICENSE`/`NOTICE` file into this document verbatim and verify
the bundled binary's checksum on every update (see the operational note in
`crates/core-engine/src/security/mod.rs` - a tampered scanner binary would
silently defeat the entire point of this module).

| Tool | License | Used for | Upstream |
|---|---|---|---|
| [Trivy](https://github.com/aquasecurity/trivy) | Apache-2.0 | Dependency/config CVE scanning feeding the code heatmap | Aqua Security |
| [GitLeaks](https://github.com/gitleaks/gitleaks) | MIT | Secret-scanning feeding the code heatmap | - |

Both licenses permit embedding/bundling here; this repo's own source is
MIT licensed (see `LICENSE`).

## Rust dependencies

This workspace's direct Rust dependencies are all MIT/Apache-2.0/BSD-class
permissive licenses at scaffold time (Tauri, sled, rusqlite, tree-sitter,
tokio, etc.). Run `cargo install cargo-license && cargo license` from the
workspace root before shipping to generate a full, current transitive
manifest - dependency licenses can change between versions, so this should
be regenerated at release time rather than trusted from this document.
