# Contributing

Run the checks in README before submitting changes. Keep application functionality in the four workspace crates; third-party session fixes belong in `vendor/ipmi` with a provenance note and a protocol regression test.

Profiles should prefer exact product/firmware matching and include command and recovery evidence. Submit a TOML file, command examples, source URLs, relevant boundary tests and a sanitized hardware report if available. Do not label simulation or another project's report as BMCForge hardware verification. Explicitly document per-device passwords rather than guessing a universal default.

Register a new built-in with `include_str!` in the profile registry and update the compatibility inventory. External profiles need no code change or recompilation. Regenerate `profiles/schema.json` when changing the profile types. Test both matching and rejection cases, especially newer firmware that no longer accepts OEM writes.

No live network discovery or real device writes should run in regular automated tests. New read/modify/write protocols require a stateful engine and recovery tests before they can be offered as control adapters.
