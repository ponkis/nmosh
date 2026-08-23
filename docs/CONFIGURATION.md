# Configuration guide

nMosh can be configured from the command line at startup and from the in-app
options overlay while it is running.

## Options overlay

Press `O` to open the overlay. The tabs cover the view, chroma key, effects,
and MIDI mappings. NDI and MIDI inputs can be reconnected without restarting
the application.

If settings have changed, closing the overlay prompts to save, discard, or
cancel. Saved settings are written to:

```text
%APPDATA%\ponkis\nMosh\settings.json
```

When `%APPDATA%` is unavailable, nMosh falls back to `nmosh-settings.json` in
the current working directory. Older settings are migrated automatically.

## View modes

- **Free camera** reacts to MIDI energy, bend, and pitch.
- **Fixed camera** stays pointed at the media plane while mesh movement remains
  active.
- **Source aspect** preserves the incoming frame's aspect ratio.
- **4:3** fits the image to a forced 4:3 frame.

Horizontal and vertical flip controls correct sources with a different input
orientation. Zoom is available both as a saved setting and a MIDI mapping.

## Chroma key

The chroma key removes a selected color before later color effects are applied.
It provides controls for tolerance, edge softness, and spill suppression.

To sample directly from the current NDI image:

1. Open the **Key** tab.
2. Select **Eyedropper**.
3. Click the video outside the options panel.

The sampled color is stored and the chroma key is enabled automatically.

## MIDI behavior

Notes drive energy, pitch, gate, and transient shock pulses. Pitch bend bends
the 3D video plane. Control Change messages drive assignable effect parameters.

MIDI learn works like a mapping panel in a DAW:

1. Open the **MIDI** tab.
2. Select **Learn** beside a parameter.
3. Move a controller or play a note.

A MIDI source can be assigned to only one target. Learning an existing source
removes its previous assignment before applying the new one.

### Default mappings

| MIDI input | Parameter |
| --- | --- |
| `CC 1` | Warp / swirl |
| `CC 2` | Chromatic split |
| `CC 7` | Brightness |
| `CC 10` | Hue rotation |
| `CC 11` | Feedback amount |
| `CC 12` | Glitch amount |
| `CC 13` | Scanlines |
| `CC 14` | Kaleidoscope |
| `CC 16` | 3D depth |
| `CC 17` | 3D rotation |
| `CC 18` | Pixelation |
| `CC 19` | Edge enhancement |
| `CC 20` | Tunnel |
| `CC 21` | Invert / solarize |
| `CC 71` | Cube morph |
| `CC 72` | White flash / strobe |
| `CC 73` | Chroma tolerance |
| `CC 74` | Zoom |
| `CC 75` | Chroma softness |
| `CC 78` | Spare mapping |
| `Note 60` | Reset effects |

nMosh continues to render NDI video when no MIDI device is connected.

## NDI runtime discovery

The runtime path is checked in this order:

1. The `--ndi-dll` command-line value.
2. The `NMOSH_NDI_DLL` environment variable.
3. A runtime library next to the nMosh executable.
4. The system library search path.
5. Standard Windows NDI Runtime and SDK installation directories.

The legacy `MIDI_NDI_DISTORTER_NDI_DLL` variable is also recognized for
compatibility, but new setups should use `NMOSH_NDI_DLL`.

If no source appears, confirm that the sender and nMosh are on the same network,
the NDI runtime is installed, and Windows Firewall permits NDI discovery and
traffic. Use `--ndi` only to select among discovered sources; it is a substring
filter, not a hostname or URL.
