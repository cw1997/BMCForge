# Configuration

`--config PATH` selects a versioned TOML file. Without it, the OS user configuration directory is selected using `directories::ProjectDirs`:

- Windows: the current user's AppData configuration directory.
- macOS: the current user's Application Support directory.
- Linux: `$XDG_CONFIG_HOME`, or the user's `.config` directory.

`bmcforge init` prints the exact path. A first TUI launch with no file starts an empty fleet; importing or saving settings creates the file. `validate-config` requires an existing file and never connects to a device.

## Guided input

The TUI opens prompts and typed forms by default. Tab/Shift+Tab moves between fields; Enter toggles a switch or opens a searchable choice list. F2/Ctrl+Enter validates and submits; Esc cancels without saving. Target prompts support multi-line paste. Password and Kg fields are masked, with F6 to show/hide the focused secret.

F7 explicitly opens advanced TOML for settings, server, credential and manual-duty forms. F7 validates the edited text before returning to the form; invalid drafts remain editable. Saving a server still waits for its worker to acknowledge reconfiguration. Manual duty requests still require a separate preview and confirmation.

## Global settings

`language` defaults to `en`; built-in IDs are `en`, `zh-CN`, `zh-TW`, `ja`, `ko`, `de`, `fr`, `es`, `pt-BR`, `it`, `ru`. Community language tags are accepted and missing packs fall back to embedded English. `theme`: `dark` or `light`; `mouse` defaults to true and enables clicks, double-clicks, local wheel scrolling and editor cursor positioning. Existing explicit preferences remain unchanged. Language and mouse changes apply immediately; concurrency changes require restarting. `concurrency` limits simultaneous device operations; device and scan concurrency must be 1..1024. See [language packs](localization.md).

Discovery defaults: `scan_concurrency = 32`, `scan_port = 623`, `scan_timeout_ms = 2000`, `scan_retries = 1`, `scan_rate_per_second = 32`, `scan_confirm_above = 4096`. Every scan has a target preview; larger scans display a large-scan notice. Inputs larger than 1,048,576 addresses are rejected before expansion, so split large networks. Configuration may reduce the confirmation threshold.

`history_capacity = 300` bounds per-device max-RPM history and activity entries. `temperature_unit` accepts `C` or `F`; `sensor_filter` selects sensor names/types by substring. `[settings.keys]` maps actions to unique single-character shortcuts; the number keys 1..5 remain reserved. The Settings form edits these bindings; clearing a binding removes it. Navigation/page action keys are reserved when assigning new bindings through the TUI. See the footer for active shortcuts. Missing keys in a fully supplied keys map are unbound; the corresponding actions remain available by mouse or in the F8 action area.

## Server records

Each `[[servers]]` has a stable `id`, `name`, `host`, `port` (623), `[servers.credentials]` username/password, optional `profile`, tags, `timeout_ms` (2000), `retries` (1), `poll_seconds` (5), `privilege` (4) and optional `kg` for two-key BMC logins. `allow_experimental` defaults to false. IDs cannot be changed through the editor. Hosts can be IPv4, IPv6 or DNS names; automatic discovery is IPv4 only.

The profile browser selects a profile for the current fleet row. Automatic matching requires an exact product name or product ID in addition to applicable vendor/firmware constraints. Ambiguous matches require explicit selection. A profile's known hardware and firmware limits are checked again before writing, including explicit selection. A family profile with no exact model criteria is offered for explicit selection only.

Optional `model_id` stores the user-confirmed catalog ID; `identity_evidence` retains the original Device ID bytes, FRU, machine types and decoded firmware at identification time. `identification_error` records a reviewed failure after redaction. Neither a model choice nor these persisted fields overrides the live identity guard before writes. See [catalog and recognition](models.md).

`retries` applies only to read/reconnect attempts. Explicit authentication failures do not retry, and background polling pauses until credentials are edited. A write uses one transport attempt. Timeouts may mean the command executed but its response was lost; the UI records this uncertainty and retains the automatic-restore obligation.

## Persistence

Passwords and optional Kg keys are plaintext, including backups. Unix temporary/new files are restricted to mode 0600. Windows files use the user's private configuration directory and inherited ACLs; when using a portable path choose appropriate directory permissions yourself. Do not commit live configuration files. Debug representations redact credentials and Kg; activity and diagnostic export also redact configured secrets.

Saving validates the complete configuration, writes and syncs a temporary file in the same directory, keeps the previous validated configuration as `.bak`, and atomically replaces the destination. Future-version or damaged files are not silently overwritten. Keep only one writer application per configuration file.

The UI does not persist a running manual-control request. Every application start begins in unknown control state and only monitors until the user requests a change. Exiting restores only devices taken over in that process; a failed restore exits with a nonzero status and names the affected server.



The `dark` and `light` values of `settings.theme` apply to all terminal surfaces, including dialogs, selection and pointer feedback. Use Ctrl+X then t to select a theme; the selection is saved immediately outside demo mode. Ctrl+P lists actions with their effective configured shortcuts. No configuration migration is required.
