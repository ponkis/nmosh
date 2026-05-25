# nMosh

by ponkis powered by ponkis.xyz

Native Rust desktop video processor for NDI video and MIDI-driven GPU distortion. nMosh receives an NDI source, listens to MIDI input, runs distortion/feedback/3D mesh effects with `wgpu`, and displays the result in a fullscreen-capable native desktop window.

Version: `1.0.0`

This project does not use Electron, web UI technology, Python, or direct Win32 calls. Windowing, GPU rendering, the options overlay, and MIDI are handled through native Rust crates.

## Requirements

- Rust stable toolchain with `cargo`
- NDI Runtime or NDI SDK installed
- `Processing.NDI.Lib.x64.dll` available in a standard NDI install folder, in `PATH`, next to the executable, or passed with `--ndi-dll`
- A GPU supported by `wgpu`
- Optional MIDI controller

## Run

```powershell
cargo run --release
```

Built executable:

```powershell
.\target\release\nmosh.exe
```

Select specific devices by substring:

```powershell
.\target\release\nmosh.exe --ndi "OBS" --midi "Launch"
```

If the app cannot find the NDI runtime automatically, pass the DLL directly:

```powershell
.\target\release\nmosh.exe --ndi-dll "C:\Program Files\NDI\NDI 6 Runtime\v6\Processing.NDI.Lib.x64.dll"
```

You can also set `NMOSH_NDI_DLL` to that DLL path.

## Controls

- `O`: open/close options
- `C`: cycle camera mode
- `1`: free camera
- `2`: fixed camera
- `F11`: toggle borderless fullscreen
- `F`: toggle borderless fullscreen
- `Esc`: close options, leave fullscreen, or quit when already windowed

The title bar shows:

```text
nMosh v1.0.0 | MIDI: {device/status} | NDI: {source/status}
```

## Options

The in-app options overlay is tabbed and scrolls inside the window. It can reconnect NDI and MIDI inputs, switch camera mode, set source or 4:3 aspect, adjust zoom, flip input orientation, configure chroma key, reset effects, and edit MIDI bindings with MIDI learn.

Camera modes:

- Free camera: current reactive 3D view
- Fixed camera: camera stays pointed at the media plane while 3D mesh movement remains active

Default video orientation correction flips vertically only. The options overlay exposes horizontal and vertical toggles if another source needs different handling. Older saved settings are migrated so the previous horizontal-flip default is not preserved accidentally.

Settings can be saved to:

```text
%APPDATA%\ponkis\nMosh\settings.json
```

If options contain unsaved changes, closing the options panel prompts to save, discard, or cancel.

## Chroma Key

The chroma key section masks a selected color from the raw NDI video before hue, thermal, chromatic split, and other color effects. Use the color picker manually, or click `Eyedropper` and then click the video outside the options panel to sample a key color from the current NDI frame.

## Added Effects

- Cube morph: smoothly transforms the NDI plane into a local-space 3D cube while keeping the existing distortion stack active
- Oscilloscope: multi-trace green CRT waveform view
- Posterize: stepped color levels
- Thermal color: heat-map style recoloring

## MIDI Mapping

Notes drive energy, pitch, gate, and shock pulses. Pitch bend bends the 3D video plane.

MIDI learn works like a DAW mapping panel: open `MIDI`, click `Learn` for a setting, then move a MIDI control. nMosh prevents duplicate bindings by removing that MIDI source from any previous assignment before applying the new one.

Default bindings:

- `CC 1`: warp/swirl
- `CC 2`: chromatic split
- `CC 7`: brightness
- `CC 10`: hue rotation
- `CC 11`: feedback amount
- `CC 12`: glitch amount
- `CC 13`: scanlines
- `CC 14`: kaleidoscope
- `CC 16`: 3D depth
- `CC 17`: 3D rotation
- `CC 18`: pixelation
- `CC 19`: edge enhancement
- `CC 20`: vignette
- `CC 21`: invert/solarize
- `CC 74`: zoom
- `CC 71`: cube morph
- `CC 72`: oscilloscope
- `CC 73`: chroma tolerance
- `CC 75`: chroma softness
- `CC 76`: posterize
- `CC 77`: thermal color
- `Note 60`: reset effects

If no MIDI device is present, nMosh still runs and shows the NDI stream with baseline GPU processing.
