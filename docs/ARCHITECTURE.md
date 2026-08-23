# Architecture

This document describes nMosh's runtime design and the responsibility of each
source module. The application intentionally remains a single Rust binary: its
components are separated by module boundaries without introducing a framework
or service layer that the current scope does not need.

## Runtime data flow

```text
NDI sender
    │
    ▼
NDI capture thread ── latest BGRA frame ──┐
                                         │
MIDI device ── callback/shared state ──┐  │
                                      ▼  ▼
                                  winit event loop
                                      │
                           settings + normalized input
                                      │
                                      ▼
                         wgpu scene/effects render pass
                                      │
                            ping-pong feedback texture
                                      │
                                      ▼
                              presentation render pass
                                      │
                                      ▼
                                egui options overlay
                                      │
                                      ▼
                                  window surface
```

The window event loop owns the application state and coordinates input, frame
uploads, rendering, and UI actions. NDI capture and MIDI callbacks communicate
through small shared-state boundaries so neither blocks rendering.

## Modules

### `main.rs`

The composition root. It parses command-line options, creates the window and
renderer, opens input devices, and drives redraws. It also handles global
keyboard shortcuts and applies actions emitted by the options overlay.

### `app.rs`

The persistent domain model. It defines application settings, camera and aspect
modes, chroma-key configuration, and MIDI bindings. `SettingsStore` owns JSON
loading and saving, including migrations from older settings formats.

Settings are plain serializable values. Runtime resources such as windows, GPU
objects, and device connections are deliberately kept out of this module.

### `ndi.rs`

The NDI boundary. The runtime library is loaded dynamically with `libloading`,
which keeps the proprietary NDI binary outside this repository and avoids a
link-time SDK dependency.

Capture runs on a dedicated worker thread. The worker discovers a matching
source, receives frames, converts supported four-character pixel formats into
packed BGRA data, and replaces a single latest-frame slot. Dropping `NdiInput`
signals the worker to stop and joins it.

### `midi.rs`

The MIDI boundary. `midir` invokes a lightweight callback that updates raw MIDI
state behind a mutex. The render loop reads normalized snapshots containing CC
values, note state, energy, pitch, aftertouch, pitch bend, and trigger counters.

### `renderer.rs`

Owns all GPU resources and rendering state. A frame consists of:

1. Uploading the latest NDI frame when one is available.
2. Smoothing MIDI-derived values and writing the uniform buffer.
3. Rendering the video mesh and effects into one of two feedback textures.
4. Presenting that texture to the window surface.
5. Drawing the options overlay on the same surface.

The two feedback textures alternate read/write roles each frame. This permits
temporal feedback without sampling from the texture currently being rendered.

### `ui.rs`

Owns the `egui` overlay and its transient state, including tabs, MIDI learn,
the chroma-key eyedropper, and unsaved-change prompts. It returns `UiAction`
values to the event loop instead of directly reconnecting devices or writing
settings.

### `shaders/effects.wgsl`

Contains the mesh vertex shader, scene fragment shader, fullscreen presentation
shader, coordinate transforms, and the visual effect functions. CPU-side
uniform structures in `renderer.rs` must remain layout-compatible with the WGSL
`Uniforms` structure.

## Platform and external boundaries

- `build.rs` embeds the Windows executable icon and is a no-op on other targets.
- NDI runtime discovery has platform-specific candidates, but Windows is the
  currently documented and supported application target.
- The NDI runtime is not distributed with nMosh. Users must install it under
  its own license.
- Application settings live outside the repository and build output.

## Design rules

- Keep unsafe FFI isolated in `ndi.rs`.
- Keep GPU resource ownership isolated in `renderer.rs`.
- Pass serializable settings across UI and renderer boundaries rather than GPU
  or window handles.
- Prefer latest-value input state over unbounded frame or MIDI queues; nMosh is
  a real-time visual application, so stale input has little value.
- Add a new persistent setting with a default, sanitization bounds, and a
  settings-version migration when compatibility requires it.
