# BMCForge

[English](README.md) | [中文](README.zh-CN.md)

Native IPMI terminal tool written in Rust + Ratatui for managing multiple servers: view fan RPM and sensors, set fixed fan duty cycles, and restore BMC automatic mode. No need to install ipmitool.

This is an early development release. Requirements include an interactive terminal and a BMC supporting IPMI 2.0 LAN/RMCP+ Suite 3. Windows Terminal, macOS Terminal, or modern Linux terminals are recommended. Cross-CPU release targets do not imply support for legacy Windows versions.

## Quick Start

After installing [Rust stable](https://rustup.rs/):

```sh
cargo run --locked -p bmcforge -- --demo
```

Demo mode is fully offline, using simulated devices for Dell, Supermicro, Lenovo, and conflicting machine types. It does not read or save real server configurations. The default discovery range `192.0.2.20-192.0.2.23` demonstrates unique identification, ambiguous selection, and final add preview.

```sh
cargo run --locked -p bmcforge -- --config bmcforge.local.toml init
cargo run --locked -p bmcforge -- --config bmcforge.local.toml
```

In the UI, press `i` to batch import, or `d` to discover IPv4 subnets. Each line accepts an IP, hostname, CIDR, or full address range (e.g., `192.0.2.10-192.0.2.20`). Discovery does not accept hostnames or IPv6 subnets. IPv4 CIDR excludes network and broadcast addresses; `/31` and `/32` are treated as point-to-point/single addresses.

Import shows a preview first; press Enter to proceed with common credentials, then F2 to save. The discovery flow is: unauthenticated probing → select devices and batch credentials → identification → handle model ambiguity → final preview confirmation before adding; identification does not require SDR success. Regular imports can save offline devices. No response means unknown, not that the device doesn't exist. Credentials are not auto-rotated on retry.

## Terminal Operations

- `1` to `5` or Tab: Servers, Sensors, Network Discovery, Adapter Library, Operation Log
- ↑ / ↓ or j / k: Navigate; Enter: View details or apply selected adapter
- Space: Multi-select; Esc: Close dialog, clear search and multi-selection
- `i`: Import; `d`: Discover/Cancel discovery; Discovery page `f`: Toggle candidates/all results
- `e`: Edit the current server form (credentials, adapter, timeouts, polling parameters)
- `m`: Set fixed duty cycle; `a`: Request auto mode; list servers/params before execution, Enter to confirm
- `p`: Adapter library; Adapter page `u`: Fill public default credentials into current server edit form (still needs saving)
- `/`: Search servers, addresses, tags; `,`: Edit settings; PgUp/PgDn: Scroll details/logs
- `x`: Export sanitized diagnostics to config directory; `q` or Ctrl+C: Restore auto mode and exit
- Delete: Restore auto mode first, then remove server; F1: Operation help
- Edit field F2 or Ctrl+Enter: Save/continue; Esc: Cancel; multi-line paste supported. F2 for terminals that can't distinguish Ctrl+Enter
- Mouse enabled by default: Click tabs, rows, checkboxes, adapters, language and action buttons; double-click server to enter details; scroll wheel only affects the hovered area. Dialogs have clickable confirm/cancel buttons
- F3 or top language entry: Switch language and save instantly; F4: Toggle mouse instantly. For terminal copy, use Shift+drag or terminal native selection
- Discovery page A: Select all visible candidates; G: Select current /24 group; `i`: Identify selected; `c`: Cancel identification. Model selection supports search; F5: Keep unknown; F6: Skip

Forms use Tab/Shift+Tab to move between fields, Enter to toggle or open a searchable option picker, and F2/Ctrl+Enter to submit. Multi-line target prompts use Enter for new lines; search accepts Enter to apply. Invalid input keeps the draft and focuses the affected field. Passwords and Kg keys are masked; F6 toggles visibility. Settings, server, credential and duty forms offer F7 for advanced TOML; F7 returns after validation, and Esc cancels the draft. IDs and identity evidence are preserved outside ordinary form fields.

Hover highlights controls without changing selection; clicks and keyboard activation briefly flash. Scanning and identification show animated progress. Buttons display their effective shortcuts. In narrow terminals, use F8 to focus the action area, arrows or Tab/Shift+Tab to reach hidden buttons, Enter to activate, and Esc/F8 to return.

Shortcuts can be customized in the Settings form (or `[settings.keys]`). Servers are grouped and sorted by first tag. On startup, only connect and monitor - no replay of previous fan control operations.

## Configuration & Adapters

Default config uses OS user config directory; specify `--config` path for portability. See [example config](examples/config.toml) and [configuration guide](docs/configuration.md). Passwords are stored in plaintext by design; config saves use atomic replacement with a temp file in the same dir and keep `.bak`, corrupted files are not auto-overwritten.

Adapters are TOML-based and extensible without recompilation:

```sh
cargo run --locked -p bmcforge -- --profiles-dir ./my-profiles validate-profiles
cargo run --locked -p bmcforge -- --profiles-dir ./my-profiles
cargo run --locked -p bmcforge -- --config bmcforge.local.toml validate-config
cargo run --locked -p bmcforge -- profile-schema
```

External adapters with the same ID must explicitly declare `override_builtin = true`. The adapter library shows command source, firmware constraints, default credential scope, and validation status. All bundled OEM write adapters are experimental and require `allow_experimental = true` to be explicitly enabled in server settings. See [adapter development](docs/profiles.md) and [compatibility](docs/compatibility.md).

New configs default to English; 11 built-in languages are supported and existing language/mouse preferences are preserved. Use `--locales-dir` to load Fluent community packs, `validate-locales` to verify; see [localization guide](docs/localization.md). The model catalog is separated from write commands; community models live under `models/` in `--profiles-dir`, validated with `validate-models`; see [identification flow & Lenovo group control](docs/models.md). Lenovo x3650 M5 IMM2 has experimental raw duty steps (no percentage conversion); other System x models are identify/monitor only.

Actual RPM comes from sensors; requested duty cycle is shown separately. Current duty cycle has no read-back implementation, so it shows as unknown. Write operation timeouts do not replay commands and status is marked uncertain; normal exit attempts to restore automatic control for devices taken over in this session. Forced termination, power loss, or network failure cannot guarantee restoration. This version has no thermal curves or background protection service.

The catalog has expanded to 164 entries: added representative model identification and monitoring for HPE, ThinkSystem, Inspur, New H3C, Huawei, XFusion, Sugon, ZTE, and Great Wall. H3C UIS 2000 G6 provides an independent experimental fan control adapter based on read-only protocol probing; identification, monitoring, and control ranges are documented separately - no guarantee all same-brand devices support fan control. See [vendor coverage & rationale](docs/vendor-coverage.md). Use `list-models --vendor HPE` or `list-models --vendor Sugon --json` to browse the catalog.

## Development & Validation

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --manifest-path vendor/ipmi/Cargo.toml --locked --no-default-features --features blocking --lib
cargo run --locked -p bmcforge -- demo-check
python -m unittest discover -s tests -v
cargo build --release --locked -p bmcforge
```

Workspace tests include command-level simulated BMC and TUI TestBackend; vendor tests use UDP loopback BMC to verify RAKP, encrypted communication, integrity checks, bad passwords, replay, and write timeouts. Simulated tests do not constitute real hardware validation. See [hardware testing process](docs/hardware-testing.md).

GitHub Actions builds, tests, and packages for Windows x64/ARM64/x86, macOS Intel/Apple Silicon, Linux musl x64/ARM64/ARMv7. Every branch runs the tests and produces debug packages whose filenames embed the full git hash and UTC build time; only updates to the `main` branch publish a release, which contains both debug and release packages. Linux ARM uses cross + QEMU; macOS uses macOS runners; Windows uses MSVC. Each package includes test method description and SHA-256. See [release process](docs/releasing.md).

## Contributing & License

For new adapters, submit source references, clear model/firmware ranges, the method to restore auto mode, and command test samples, following [contributing guidelines](CONTRIBUTING.md). Do not assume same-brand or IPMI support equals fan control support.

BMCForge is licensed under [Apache-2.0](LICENSE). The native session implementation is based on ipmi 0.1.2 maintained in-repo; see [third-party sources and modifications](vendor/ipmi/BMCFORGE-PATCHES.md).

Ctrl+P opens the searchable command palette with effective shortcuts and unavailable reasons. After Ctrl+X, press p for commands, 1–5 for pages, t for themes, h for help, or q for normal shutdown. The prefix expires after two seconds; Esc cancels it. Existing shortcuts remain available, and text inputs retain their editing keys. Wide terminals show fleet details alongside the list; below 80 columns, Enter opens details in a dialog.

Dialogs use shared shadows, rounded borders and dimmed backgrounds. The full-width reverse-video status strip opens the complete redacted message when clicked; use Shift+drag or native terminal selection to copy. Click Ctrl+P for commands or ⋯ for contextual actions; cancelling an action menu restores the original form and draft. Click scrollbars to navigate; the wheel scrolls the pane under the pointer. Mouse support defaults to enabled, respects saved preferences, and can be toggled with F4.
