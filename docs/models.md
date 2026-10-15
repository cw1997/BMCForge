# Model catalog and identification

The identity catalog in `profiles/models/` is independent of fan command adapters. Recognition never proves fan-write compatibility. Models without a sourced write/restore sequence remain monitor-only. Dell R210 II deliberately has no whole-system fan adapter. Dell 11–14G R/T entries include canonical PowerEdge names and Dell FRU aliases. Lenovo/IBM entries include x3250, x3500, x3550 and x3650 M3/M4/M5 names and sourced machine types.

Each catalog TOML has `schema_version = 1` and one or more `[[models]]` records: stable `id`, `vendor`, canonical `name`, `aliases`, `manufacturer_ids`, optional `product_ids`, `machine_types`, optional `profile` ID and HTTPS `sources`. `profiles/models/schema.json` describes the parsed TOML structure. Add community files under `--profiles-dir DIRECTORY/models/`; existing top-level fan adapter files continue to work. Replacing an existing model requires file-level `override_builtin = true`. Duplicate external IDs are rejected. Empty and unknown product IDs do not create matches.

Matching normalizes case and punctuation while preserving original evidence. An explicit name/FRU alias or machine type takes priority over product-only candidates. Conflicting explicit evidence is kept ambiguous. Shared product IDs generate multiple candidates. A manufacturer alone, an IP address, missing FRU or failed authentication never supplies a model guess. Machine-type evidence comes from FRU product name/part fields and separately retained machine types, not arbitrary serial-number suffixes.

Discovery workflow:

1. Enter IPv4 addresses/ranges/CIDRs; review the expanded count and start unauthenticated IPMI probing.
2. Select candidate checkboxes; A selects all visible candidates, G selects the current IPv4 /24 group. Filtering does not hide or discard an explicit selection.
3. Choose **Identify selected** (`i`) and enter one credential set for that batch. Only those credentials are attempted. Cancel identification with its Cancel button or `c`.
4. Identity reads authenticate and request Device ID plus FRU; SDR failures cannot block identification. Review unknown/failed results and searchable ambiguous models. Enter selects, F5 keeps unknown, F6 skips. Raw FRU, machine types, original Device ID bytes and firmware remain separate from the user's choice.
5. Review endpoint, chosen model, recognition state and control capability. Confirm to save. Existing normalized address+port entries are skipped; their credentials are never replaced. Repeat with different credentials for another batch or explicit retry.

Canceled task generations cannot update current identification/discovery results. Authentication failures are shown without rotating factory defaults. Keeping an unknown or authentication-failed endpoint is explicit and permits later credential editing/monitoring; it does not enable fan writes. Saved `model_id`, `identity_evidence` and `identification_error` preserve the reviewed record. Live observations remain separate.

Manual model selection does not modify the hardware evidence used by command guards. Manufacturer, firmware, exact hardware evidence and experimental opt-in are still enforced immediately before writes. Conflicting explicit machine types disable the associated built-in adapter. Raw Get Device ID bytes, including auxiliary firmware bytes, are retained; the standard major/BCD minor revision is decoded only when BCD is valid. Auxiliary vendor bytes are not guessed into a dotted version. Missing/invalid firmware cannot satisfy a bounded adapter matcher.

## Lenovo x3650 M5 IMM2

The experimental `lenovo-x3650-m5-imm2` adapter independently implements protocol facts from [the public control/recovery implementation](https://github.com/laingcc/Lenovo-M3650-Nvidia-GPU-Fan-Control). The GPL script is not copied or bundled. Its evidence is a community implementation, not BMCForge hardware verification, nor a claim about every IMM2 firmware.

Manual bank writes are `3a 07 <bank> <raw> 01`; automatic restore is `3a 07 <bank> 12 00`. Bank 1 is CPU1/front; bank 2 is CPU2/rear. The sourced levels are `0x12`, `0x35`, `0x40`, `0x50`, `0x60`, `0x70`, `0x80`, `0x90`, `0xa0`, `0xff`. These are non-linear raw control levels. The UI displays hexadecimal requests and separately measured RPM, and never converts them to an unverified percentage.

Enable `allow_experimental = true` explicitly for the device, then `m` opens:

```toml
raw = 64 # 0x40
zone = 1
```

Use exactly one of `raw` and `percent`; percentages are rejected by this adapter, and raw values are rejected by percentage adapters. Normal exit restores only the banks this process attempted to take over. Recovery is recorded before writes, including timed-out/partially successful writes. Recovery attempts continue for other banks after one fails, retain failed bank obligations and report failures. An explicit automatic request without a takeover targets both configured banks. Device-scoped Dell/Supermicro restoration is unchanged.

Run `validate-models`, `validate-profiles`, `demo-check` and the workspace tests. Submit source links for each alias/machine type and rejection cases for mismatching hardware. Never promote simulation to hardware verification. Other System x models remain monitoring-only until complete control and automatic recovery evidence is available.
