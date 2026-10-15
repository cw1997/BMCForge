# TOML profile format, version 1

Copy a profile from `profiles/` into a separate directory and pass `--profiles-dir`. Built-ins are embedded in the binary. Invalid files fail loading with a filename and validation error. External files may override a built-in only with `override_builtin = true`; duplicate external IDs are rejected.

`profiles/schema.json` is the generated JSON Schema for the parsed TOML data. Regenerate it with `bmcforge profile-schema`. Runtime validation additionally checks command examples, ranges, sequence presence and firmware ordering.

Required metadata: `schema_version`, unique `id`, `vendor`, `description`, `verification`, HTTPS `sources`, `notes`, and `[matcher]`.

Verification values:

- `hardware_verified`: requires a documented BMCForge hardware report for the exact model and firmware.
- `documented`: official documentation or an implementation source supports the operation; no implied hardware test.
- `experimental`: commands are available only when the individual server enables experimental control.
- `unsupported`: catalog/monitoring entry with no fan write commands.

`[matcher]` optionally contains `manufacturer_id`, `manufacturer_ids` (an allowlist), `product_ids`, exact `models`, `excluded_models`, and numeric dotted `firmware_min` / `firmware_max`. All specified criteria must match. Vendor-only entries cannot auto-select a command family. Missing firmware does not satisfy a bounded firmware range.

`[[default_credentials]]` accepts `username`, optional `password`, HTTPS `source` and an applicability `note`. Omit password for devices using a unique label password. Defaults are never automatically attempted during network scanning. The UI can prefill them in an explicit edit form.

## Fan operations

Optional `[fan]` contains `min_percent`, `max_percent`, `scale`, `zones`, and `manual`, `set`, `automatic` step arrays. `scale` is the raw byte for 100% (usually 100 or 255). Zones expose only IDs supported by that adapter; 255 is not universally an all-zone ID.

Each step has numeric `netfn` and `command`, `data` as string tokens, optional `delay_ms` (at most 30000), and optional `expect` bytes for an exact response check. Data tokens accept decimal/hex bytes, `$percent`, `$zone`, and `$duty`. Duty conversion is rounded as `(percent * scale + 50) / 100`.

A manual request executes `manual` followed by `set`; restoring executes `automatic`. The complete sequence is serialized per server. A failed step stops the sequence and is not replayed automatically. The restore sequence is retained even if the first manual command timed out. Changing profiles while a device is under application control requires restoring automatic first.

`percent_values` optionally restricts control to a finite set of documented percentages. The UI initializes to a supported value and lists allowed levels. `raw_values` instead selects typed raw control; `$raw` preserves the byte without percentage conversion. A request cannot cross the percent/raw boundary. Raw and percentage level lists cannot be mixed. `restore_per_zone = true` records recovery separately for every attempted zone; otherwise recovery remains device-scoped.

`probe` optionally supplies reviewed **read-only** protocol checks before the first takeover. Every probe must have an expected response. A failed or mismatched probe stops before any manual/set writes and creates no takeover obligation. The engine cannot infer OEM command semantics, so contributors must supply read-only command evidence. Probes run after live identity checks and use the requested value/zone when compiling tokens. These checks establish protocol availability; they do not fabricate a complete vendor firmware version or bypass a configured firmware bound.

Steps can use `expect_data` (string tokens compiled with the same parameters as request data) instead of fixed `expect` bytes. They are mutually exclusive. This allows checking a vendor header and echoed PWM byte exactly. See `h3c-uis2000-g6.toml`: only 20/40/60/80/100 percent are documented, its OEM signature is checked before writes, and automatic mode has its own checked response. This is separate from UniServer HDM policy commands.

Every control profile needs `[[examples]]`: input percent/zone and `expected` command arrays, each containing netfn, command and payload. Profile loading verifies these arrays against compilation. Include normal and boundary cases in new contributions, plus a restore test in the Rust test suite if restoration behavior differs.

The format intentionally cannot execute shell commands or scripts. Stateful protocols that require reading current duty, register-dependent bit operations or BMC-stack discovery need engine extensions and tests; do not approximate those with unsafe fixed templates.

## Submission

Provide exact model/BMC/firmware evidence, sources for every OEM command, defaults and their applicability, affected zones, scaling, delays, minimum duty, and a recovery procedure. Public source evidence is not hardware verification. Remove passwords, serial numbers and sensitive network addresses from reports. See `docs/hardware-testing.md`.
