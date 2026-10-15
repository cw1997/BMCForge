# Vendored ipmi 0.1.2

Source: https://crates.io/crates/ipmi/0.1.2 and https://github.com/lvillis/ipmi-rs

The upstream Cargo package declares MIT licensing. Original source attribution is
retained here; this directory is a third-party component, separate from the
BMCForge Apache-2.0 application. The original package did not include a LICENSE
file. See Cargo.toml for upstream dependency licenses.

BMCForge modifications:

- Refuse automatic cipher downgrade: only RAKP-HMAC-SHA1, HMAC-SHA1-96 and
  AES-CBC-128 (suite 3) are negotiated. Suite 17 and IPMI 1.5 are not implemented.
- Require response security flags to match the negotiated session.
- Reject zero/replayed/out-of-order incoming session sequences.
- Disable inherited `IPMI_DEBUG` raw packet dumps; diagnostics use the application's redaction layer.
- Add a loopback UDP BMC for authenticated/encrypted protocol regression tests.

The application uses the blocking API on Tokio blocking threads; network work
does not run on the TUI event loop. All requests have exactly one transport
attempt. Application read retries reconnect; writes are never automatically
replayed.

Run protocol tests with:

    cargo test --manifest-path vendor/ipmi/Cargo.toml --no-default-features --features blocking --lib
