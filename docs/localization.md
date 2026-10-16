# Community language packs

New configurations use English and enable the mouse. Explicit saved language and mouse values remain unchanged. Use the native-name menu at the top or F3 to switch language immediately. Change the saved mouse preference in Settings. Both choices persist. Shift+drag or the terminal's native selection/copy shortcut copies text while mouse capture is enabled; terminal behavior varies.

Embedded packs are English, 简体中文, 繁體中文, 日本語, 한국어, Deutsch, Français, Español, Português (Brasil), Italiano and Русский. Their Fluent message IDs and variable sets must match English exactly. Please submit language corrections: completeness checks establish resource contracts, not native-speaker linguistic review.

Each community pack is a folder inside `--locales-dir DIRECTORY`:

```text
my-locales/
  nl/
    locale.toml
    main.ftl
```

`locale.toml`:

```toml
schema_version = 1
id = "nl"
name = "Nederlands"
```

`main.ftl`:

```ftl
fleet = Servers
ready = Gereed
device-count = { $count ->
    [one] { $count } apparaat
   *[other] { $count } apparaten
}
```

Missing messages fall back to the embedded English pack, including when partially overriding English itself. To replace a built-in language ID, explicitly add `override_builtin = true` to the metadata. Unknown IDs, malformed Fluent syntax, duplicate IDs and variable mismatches are reported and that external pack is ignored. One bad pack does not prevent other packs from loading. A missing/unreadable external directory also produces a diagnostic. Configurations selecting an unavailable language display English without losing the saved preference.

```sh
cargo run --locked -p bmcforge -- --locales-dir ./my-locales validate-locales
cargo run --locked -p bmcforge -- --locales-dir ./my-locales --demo
cargo run --locked -p bmcforge -- locale-schema
```

The schema in `locales/schema.json` validates metadata parsed from TOML. Runtime validation additionally checks language tags, Fluent patterns and message/variable contracts. Use the stable keys in `locales/en/main.ftl`; translate values, preserve `$variable` names, and use Fluent select expressions for locale plural rules. Avoid renaming IDs when updating wording. Pack names must use their native script. Technical BMC labels, hardware evidence, adapter descriptions, byte sequences and third-party error details retain their original text.

Background events store message IDs, arguments and separate technical details. Rendering happens in the currently selected locale, so earlier structured events change language too. Terminal layouts measure displayed Unicode width for navigation and editor hit testing. Run the workspace tests before submitting a pack and inspect long strings in small terminals as well as CJK text.
