# Build and release

`ci.yml` runs formatting, Clippy, profile/schema/config validation, packaging unit tests, application tests and native protocol tests on three operating systems.

`build.yml` builds and tests eight targets on pull requests, main/master pushes, manual dispatch, and calls from the release workflow:

- Windows MSVC: `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`, `i686-pc-windows-msvc`.
- macOS: `x86_64-apple-darwin`, `aarch64-apple-darwin` using macOS runners.
- Linux musl: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `armv7-unknown-linux-musleabihf` using cross 0.2.5 containers. ARM executions use QEMU.

Windows x86 is built on the x64 runner and executed using WOW64. Windows ARM64 uses an ARM64 runner. Architecture targets do not promise support for obsolete operating systems. There is no code signing/notarization in this first version.

Each target runs workspace tests, the vendored protocol tests, and the release executable's offline `demo-check`. Packaging verifies PE/ELF/Mach-O architecture before producing a ZIP (Windows) or tar.gz (Unix), SHA-256 sidecar, and machine-readable build/test status. Status identifies native/container/QEMU execution separately and always leaves hardware verification false.

For local cross compilation, install Rust target components, Docker and cross, then use the same commands as the build workflow. macOS builds require a macOS SDK/runner; the workflow does not pretend that one Linux host can produce every platform binary without platform tooling.

To release, update the workspace Cargo version and changelog, then push a matching `vX.Y.Z` tag. The release workflow first runs all eight builds/tests; it checks tag/version consistency, requires every target's artifacts and checksums, and creates a **draft** release. Only that final job receives contents write permission. Workflow dispatch builds artifacts but does not publish a tagless release. Review the draft's test status and compatibility notes before publishing it.

Run `python scripts/package_release.py --target TARGET --test-mode MODE` only after completing the corresponding build, tests and executable smoke test; the script packages that already-verified output. It does not execute the binary itself.
