<p align="center">
  <img src="src/icon.png" alt="nMosh icon" width="112">
</p>

<h1 align="center">nMosh</h1>

<p align="center">
  Native NDI video processing with MIDI-controlled GPU effects.
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-ed6a2c?logo=rust&logoColor=white">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-0078d4?logo=windows&logoColor=white">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/github/license/ponkis/nmosh"></a>
</p>

nMosh receives an NDI video source, translates MIDI input into real-time control
signals, and renders distortion, feedback, chroma-key, and 3D mesh effects with
`wgpu`. It runs as a native desktop application with a fullscreen output and an
in-app configuration overlay.

## Features

- Discovers and receives NDI video without linking the NDI SDK at build time.
- Renders a GPU-native effects pipeline with feedback, glitch, chromatic split,
  kaleidoscope, pixelation, edge enhancement, scanlines, and color controls.
- Morphs video between a plane, reactive 3D mesh, cube, and tunnel views.
- Maps MIDI CC, notes, pitch bend, and aftertouch to visual parameters.
- Includes MIDI learn with duplicate-binding protection.
- Provides chroma-key controls with an on-canvas eyedropper.
- Saves versioned settings and migrates older settings automatically.
- Works without a MIDI device for clean NDI monitoring and manual control.

## Requirements

- Windows 10 or newer.
- A current stable [Rust toolchain](https://www.rust-lang.org/tools/install) when
  building from source.
- The [NDI Runtime or NDI SDK](https://ndi.video/for-developers/ndi-sdk/download/).
- A GPU and driver supported by `wgpu`.
- An NDI sender on the local network.
- A MIDI controller is optional.

## Quick start

Clone the repository and build an optimized executable:

```powershell
git clone https://github.com/ponkis/nmosh.git
cd nmosh
cargo build --release
```

Run nMosh:

```powershell
.\target\release\nmosh.exe
```

nMosh selects the first available NDI source and MIDI port by default. Select
devices by a case-insensitive name substring when a setup has multiple inputs:

```powershell
.\target\release\nmosh.exe --ndi "OBS" --midi "Launch"
```

If the NDI runtime is not discovered automatically, provide it explicitly:

```powershell
.\target\release\nmosh.exe --ndi-dll "C:\Program Files\NDI\NDI 6 Runtime\v6\Processing.NDI.Lib.x64.dll"
```

The same path can be set with the `NMOSH_NDI_DLL` environment variable.

## Command line

```text
Usage: nmosh [--ndi SOURCE_SUBSTRING] [--ndi-dll PATH]
             [--midi PORT_SUBSTRING] [--width PX] [--height PX]
```

| Option | Description |
| --- | --- |
| `-h`, `--help` | Print command-line usage. |
| `--ndi <name>` | Prefer an NDI source whose name contains the value. |
| `--ndi-dll <path>` | Load the NDI runtime from an explicit path. |
| `--midi <name>` | Prefer a MIDI port whose name contains the value. |
| `--width <px>` | Set the initial window width; defaults to `1280`. |
| `--height <px>` | Set the initial window height; defaults to `720`. |

## Keyboard controls

| Key | Action |
| --- | --- |
| `O` | Open or close the options overlay. |
| `C` | Cycle the camera mode. |
| `1` | Select the free camera. |
| `2` | Select the fixed camera. |
| `F` / `F11` | Toggle borderless fullscreen. |
| `Esc` | Close options, leave fullscreen, or quit when windowed. |

Settings are stored at `%APPDATA%\ponkis\nMosh\settings.json`. See the
[configuration guide](docs/CONFIGURATION.md) for effects, chroma key, MIDI
bindings, and runtime discovery details.

## Project layout

```text
src/
├── main.rs                Application entry point and event loop
├── app.rs                 Settings, migrations, and MIDI binding model
├── ndi.rs                 Runtime NDI loading, discovery, and frame capture
├── midi.rs                MIDI connection and normalized controller state
├── renderer.rs            wgpu resources, render passes, and effect state
├── ui.rs                  egui options overlay and user actions
└── shaders/effects.wgsl   Mesh, effects, feedback, and presentation shaders
docs/                      Architecture and configuration documentation
build.rs                   Windows resource embedding
wresources.rc              Windows executable icon resource
```

For the runtime data flow and module responsibilities, read
[Architecture](docs/ARCHITECTURE.md).

## Contributing

Bug reports and focused improvements are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request, use the issue
templates for reports and requests, and follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Security issues should be reported
privately as described in [SECURITY.md](SECURITY.md).

## License

nMosh is available under the [MIT License](LICENSE).

NDI is a trademark of Vizrt NDI AB. This project is independent and is not
affiliated with or endorsed by Vizrt NDI AB.
