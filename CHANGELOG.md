# Changelog

## Unreleased

### Added

- Clickable TUI controls, double-click details, pane-local scrolling, wide-character cursor positioning and immediate mouse toggling.
- Eleven embedded Fluent language packs, native-name switching, external packs, validation, English fallback and structured background events.
- Independent Dell PowerEdge and IBM/Lenovo System x identity catalogs; cancellable identity-only batches, evidence-preserving model selection and final import review.
- Experimental x3650 M5 IMM2 typed raw fan-bank control with per-bank recovery, including partial-write and recovery-failure handling.
- Locale/model schemas, contribution guides, offline ambiguous-model demo and recursive release resources.

- Native IPMI 2.0 fleet monitoring, fixed fan duty and BMC automatic-mode restore.
- Rust/Ratatui bilingual terminal interface, multi-server credentials, bulk address import and cancellable IPv4 discovery.
- Versioned TOML configuration and adapters, explicit experimental controls, evidence and command-example validation.
- Offline demo, sensor history, redacted diagnostics, and loopback protocol regression tests.
- Eight-target build/test/package workflows and draft releases with checksums and verification metadata.

### Security

- Require encrypted/authenticated RMCP+ sessions and reject response downgrades and replayed session sequences.
- Never replay OEM write sequences after uncertain completion; retain restore obligations after partial writes.
- Redact configured secrets in activity/diagnostics; keep configuration and backups local with plaintext storage documented.
