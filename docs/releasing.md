# Build and release

`verify.yml` is the single pipeline: it runs formatting, Clippy, profile/schema/config validation, packaging unit tests, application tests and native protocol tests on three operating systems, then gates every build behind those checks.

`build.yml` is only invoked by `verify.yml` through `workflow_call` — it no longer has its own triggers, so no build can bypass the checks. It builds and tests eight targets and produces a **debug** package by default or a **release** package when called with `build_type: release`. Debug archive names embed the full git commit hash and a UTC build timestamp (`bmcforge-VERSION-FULLHASH-YYYYMMDD-HHMMSS-debug-TARGET`), so artifacts from different branches never collide:

- Windows MSVC: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`, `i686-pc-windows-msvc`.
- macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin` using macOS runners.
- Linux musl: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `armv7-unknown-linux-musleabihf` using cross 0.2.5 containers. ARM executions use QEMU.

Windows x86 is built on the x64 runner and executed using WOW64. Windows ARM64 uses an ARM64 runner. Architecture targets do not promise support for obsolete operating systems. There is no code signing/notarization in this first version.

Each target runs workspace tests, the vendored protocol tests, and the built executable's offline `demo-check`. Packaging verifies PE/ELF/Mach-O architecture before producing a ZIP (Windows) or tar.gz (Unix), SHA-256 sidecar, and machine-readable build/test status. Status identifies native/container/QEMU execution separately and always leaves hardware verification false.

For local cross compilation, install Rust target components, Docker and cross, then use the same commands as the build workflow. macOS builds require a macOS SDK/runner; the workflow does not pretend that one Linux host can produce every platform binary without platform tooling.

`verify.yml` orders the work strictly: `quality` → `native-tests` → the eight-target **debug** builds on every ref. On `main`/`master` updates (and manual dispatch from those refs) it additionally runs the eight-target **release** builds serially after the debug ones, then, in the single job that receives contents write permission, requires every release target's artifacts and checksums and publishes a GitHub release containing both the debug and release packages. Pull requests and non-main branches stop after the debug builds; `release-builds` and `publish` are skipped. The release tag is derived from the workspace version plus a UTC timestamp (`vX.Y.Z-build.YYYYMMDDHHMMSS`), so every main update produces a distinct release. Update the workspace Cargo version and changelog when you want a new version line; otherwise the version component stays the same.

Run `python scripts/package_release.py --target TARGET --test-mode MODE [--build-type debug|release]` only after completing the corresponding build, tests and executable smoke test; the script packages that already-verified output. It does not execute the binary itself.
