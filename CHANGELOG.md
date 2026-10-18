# Changelog

## Unreleased

### Added

- Expanded the sourced identity catalog to 164 entries, adding HPE, ThinkSystem, Inspur, H3C, Huawei, xFusion, Sugon, ZTE and China Greatwall models with separate monitoring profiles.
- Experimental H3C UIS 2000 G6 control with a read-only protocol probe, five documented PWM levels, checked response templates and automatic recovery; UniServer HDM remains monitoring-only.
- Alphanumeric ThinkSystem machine-type matching, vendor-filtered `list-models`, model lists in adapter details and a generated per-model compatibility inventory.

- Clickable TUI controls, double-click details, pane-local scrolling, wide-character cursor positioning and immediate mouse toggling.
- Eleven embedded Fluent language packs, native-name switching, external packs, validation, English fallback and structured background events.
- Independent Dell PowerEdge and IBM/Lenovo System x identity catalogs; cancellable identity-only batches, evidence-preserving model selection and final import review.
- Experimental x3650 M5 IMM2 typed raw fan-bank control with per-bank recovery, including partial-write and recovery-failure handling.
- Locale/model schemas, contribution guides, offline ambiguous-model demo and recursive release resources.

- Native IPMI 2.0 fleet monitoring, fixed fan duty and BMC automatic-mode restore.
- Rust/Ratatui bilingual terminal interface, multi-server credentials, bulk address import and cancellable IPv4 discovery.
- Versioned TOML configuration and adapters, explicit experimental controls, evidence and command-example validation.
- Offline demo, sensor history, redacted diagnostics, and loopback protocol regression tests.
- Eight-target build/test/package workflows and releases with checksums and verification metadata.

### Changed

- `README.md` is now the pure-English README; the Chinese documentation moved to `README.zh-CN.md`, with both files cross-linking each other's language at the top.
- CI runs tests on every branch, and the multi-platform build workflow now produces debug packages by default with the full git commit hash and a UTC build timestamp in each archive name.
- Releases are published only for `main`/`master` updates and include both the debug and release packages.
- The pipeline is now a single ordered `verify.yml`: quality checks gate native tests, which gate the eight-target debug builds on every ref; `main`/`master` additionally runs the release builds serially after the debug ones before publishing. `build.yml` is reusable-only and `release.yml` is gone.

### Security

- Require encrypted/authenticated RMCP+ sessions and reject response downgrades and replayed session sequences.
- Never replay OEM write sequences after uncertain completion; retain restore obligations after partial writes.
- Redact configured secrets in activity/diagnostics; keep configuration and backups local with plaintext storage documented.
