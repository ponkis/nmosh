# Contributing to nMosh

Thanks for taking the time to improve nMosh. Bug reports, documentation fixes,
and focused pull requests are welcome.

## Before you start

- Search existing issues before opening a new one.
- Use the bug report or feature request form so the relevant environment details
  are included.
- Report security problems privately according to [SECURITY.md](SECURITY.md).
- Keep proposals aligned with nMosh's purpose: real-time NDI visuals controlled
  through a native desktop UI and MIDI.

## Development setup

You need a current stable Rust toolchain and a supported GPU. The NDI runtime is
needed to exercise live video capture but is not required for compilation.

```powershell
git clone https://github.com/ponkis/nmosh.git
cd nmosh
cargo build
```

To run against specific devices:

```powershell
cargo run --release -- --ndi "source name" --midi "port name"
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing module
boundaries, the NDI FFI layer, or GPU resource ownership.

## Quality checks

Run these checks before submitting a pull request:

```powershell
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Changes to rendering or device behavior should also be tested manually with a
live NDI sender. When relevant, test with and without a MIDI device, resize the
window, toggle fullscreen, reconnect inputs, and reopen saved settings.

## Change guidelines

- Keep unsafe code contained within the NDI boundary unless there is a strong
  technical reason to do otherwise.
- Do not commit the NDI runtime, SDK binaries, local settings, or build output.
- Keep CPU uniform structures and WGSL uniform layouts synchronized.
- Give persistent settings safe defaults and bounded values. Preserve existing
  settings through a migration when their serialized shape changes.
- Update the README or files under `docs/` when user-visible behavior changes.
- Keep pull requests focused and explain how the change was verified.

## Pull requests

A useful pull request description includes:

- the problem being solved;
- the approach and any important trade-offs;
- manual and automated verification performed;
- screenshots or a short capture for visual changes;
- related issue numbers, when applicable.

By participating in this project, you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
