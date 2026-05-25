use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "nMosh";
pub const APP_CREDIT: &str = "by ponkis powered by ponkis.xyz";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const SETTINGS_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraMode {
    Free,
    FixedPlane,
}

impl CameraMode {
    pub fn as_uniform(self) -> f32 {
        match self {
            Self::Free => 0.0,
            Self::FixedPlane => 1.0,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Free => Self::FixedPlane,
            Self::FixedPlane => Self::Free,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectMode {
    Source,
    FourThree,
}

impl AspectMode {
    pub fn as_uniform(self) -> f32 {
        match self {
            Self::Source => 0.0,
            Self::FourThree => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub settings_version: u32,
    pub camera_mode: CameraMode,
    pub aspect_mode: AspectMode,
    pub input_flip_x: bool,
    pub input_flip_y: bool,
    pub zoom: f32,
    pub cube_amount: f32,
    pub oscilloscope_amount: f32,
    pub posterize_amount: f32,
    pub thermal_amount: f32,
    pub chroma_key: ChromaKeySettings,
    pub midi_bindings: MidiBindings,
}

impl AppSettings {
    pub fn sanitized(mut self) -> Self {
        self.settings_version = SETTINGS_VERSION;
        self.zoom = self.zoom.clamp(0.25, 4.0);
        self.cube_amount = self.cube_amount.clamp(0.0, 1.0);
        self.oscilloscope_amount = self.oscilloscope_amount.clamp(0.0, 1.0);
        self.posterize_amount = self.posterize_amount.clamp(0.0, 1.0);
        self.thermal_amount = self.thermal_amount.clamp(0.0, 1.0);
        self.chroma_key = self.chroma_key.sanitized();
        self
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            settings_version: SETTINGS_VERSION,
            camera_mode: CameraMode::Free,
            aspect_mode: AspectMode::Source,
            input_flip_x: false,
            input_flip_y: true,
            zoom: 1.0,
            cube_amount: 0.0,
            oscilloscope_amount: 0.0,
            posterize_amount: 0.0,
            thermal_amount: 0.0,
            chroma_key: ChromaKeySettings::default(),
            midi_bindings: MidiBindings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromaKeySettings {
    pub enabled: bool,
    pub color: [f32; 3],
    pub tolerance: f32,
    pub softness: f32,
    pub spill: f32,
}

impl ChromaKeySettings {
    fn sanitized(mut self) -> Self {
        for channel in &mut self.color {
            *channel = channel.clamp(0.0, 1.0);
        }
        self.tolerance = self.tolerance.clamp(0.0, 1.0);
        self.softness = self.softness.clamp(0.001, 1.0);
        self.spill = self.spill.clamp(0.0, 1.0);
        self
    }
}

impl Default for ChromaKeySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.0, 1.0, 0.0],
            tolerance: 0.22,
            softness: 0.12,
            spill: 0.35,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiBindings {
    controls: [MidiBinding; MIDI_CONTROL_COUNT],
    pub reset_effects: MidiBinding,
}

impl MidiBindings {
    pub fn binding(self, control: MidiControl) -> MidiBinding {
        self.controls[control as usize]
    }

    pub fn bindings(&self) -> &[MidiBinding; MIDI_CONTROL_COUNT] {
        &self.controls
    }

    pub fn set_binding(&mut self, control: MidiControl, binding: MidiBinding) {
        self.controls[control as usize] = binding;
    }
}

impl Default for MidiBindings {
    fn default() -> Self {
        Self {
            controls: [
                MidiBinding::cc(1),
                MidiBinding::cc(2),
                MidiBinding::cc(7),
                MidiBinding::cc(10),
                MidiBinding::cc(11),
                MidiBinding::cc(12),
                MidiBinding::cc(13),
                MidiBinding::cc(14),
                MidiBinding::cc(16),
                MidiBinding::cc(17),
                MidiBinding::cc(18),
                MidiBinding::cc(19),
                MidiBinding::cc(20),
                MidiBinding::cc(21),
                MidiBinding::cc(74),
                MidiBinding::cc(71),
                MidiBinding::cc(72),
                MidiBinding::cc(73),
                MidiBinding::cc(75),
                MidiBinding::cc(76),
                MidiBinding::cc(77),
                MidiBinding::cc(78),
            ],
            reset_effects: MidiBinding::note(60),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiBinding {
    pub source: MidiSource,
    pub number: u8,
}

impl MidiBinding {
    pub const fn none() -> Self {
        Self {
            source: MidiSource::None,
            number: 0,
        }
    }

    pub const fn cc(number: u8) -> Self {
        Self {
            source: MidiSource::ControlChange,
            number,
        }
    }

    pub const fn note(number: u8) -> Self {
        Self {
            source: MidiSource::Note,
            number,
        }
    }

    pub fn label(self) -> String {
        match self.source {
            MidiSource::None => "unassigned".to_string(),
            MidiSource::ControlChange => format!("CC {}", self.number),
            MidiSource::Note => format!("Note {}", self.number),
        }
    }

    pub fn value_from(self, cc: &[f32; 128]) -> f32 {
        match self.source {
            MidiSource::ControlChange => cc[self.number.min(127) as usize],
            MidiSource::Note | MidiSource::None => 0.0,
        }
    }

    pub fn matches(self, event: MidiEvent) -> bool {
        self.source == event.source && self.number == event.number
    }
}

impl Default for MidiBinding {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiSource {
    #[default]
    None,
    ControlChange,
    Note,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiEvent {
    pub source: MidiSource,
    pub number: u8,
    pub value: f32,
    pub counter: u64,
}

impl MidiEvent {
    pub fn as_binding(self) -> MidiBinding {
        MidiBinding {
            source: self.source,
            number: self.number,
        }
    }
}

pub const MIDI_CONTROL_COUNT: usize = 22;

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiControl {
    Warp = 0,
    Chroma = 1,
    Brightness = 2,
    Hue = 3,
    Feedback = 4,
    Glitch = 5,
    Scanlines = 6,
    Kaleidoscope = 7,
    Depth = 8,
    Rotation = 9,
    Pixelate = 10,
    Edge = 11,
    Vignette = 12,
    Invert = 13,
    Zoom = 14,
    Cube = 15,
    Oscilloscope = 16,
    ChromaTolerance = 17,
    ChromaSoftness = 18,
    Posterize = 19,
    Thermal = 20,
    Spare = 21,
}

pub const MIDI_CONTROLS: [MidiControl; MIDI_CONTROL_COUNT] = [
    MidiControl::Warp,
    MidiControl::Chroma,
    MidiControl::Brightness,
    MidiControl::Hue,
    MidiControl::Feedback,
    MidiControl::Glitch,
    MidiControl::Scanlines,
    MidiControl::Kaleidoscope,
    MidiControl::Depth,
    MidiControl::Rotation,
    MidiControl::Pixelate,
    MidiControl::Edge,
    MidiControl::Vignette,
    MidiControl::Invert,
    MidiControl::Zoom,
    MidiControl::Cube,
    MidiControl::Oscilloscope,
    MidiControl::ChromaTolerance,
    MidiControl::ChromaSoftness,
    MidiControl::Posterize,
    MidiControl::Thermal,
    MidiControl::Spare,
];

pub const MIDI_CONTROL_LABELS: [&str; MIDI_CONTROL_COUNT] = [
    "Warp / swirl",
    "Chromatic split",
    "Brightness",
    "Hue rotation",
    "Feedback",
    "Glitch",
    "Scanlines",
    "Kaleidoscope",
    "3D depth",
    "3D rotation",
    "Pixelation",
    "Edge enhancement",
    "Vignette",
    "Invert / solarize",
    "Zoom",
    "Cube morph",
    "Oscilloscope",
    "Chroma tolerance",
    "Chroma softness",
    "Posterize",
    "Thermal color",
    "Spare",
];

#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn default_path() -> PathBuf {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata)
                .join("ponkis")
                .join("nMosh")
                .join("settings.json");
        }

        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("nmosh-settings.json")
    }

    pub fn new() -> Self {
        Self {
            path: Self::default_path(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        let data = fs::read_to_string(&self.path)
            .map_err(|error| format!("could not read {}: {error}", self.path.display()))?;
        let value = serde_json::from_str::<serde_json::Value>(&data)
            .map_err(|error| format!("could not parse {}: {error}", self.path.display()))?;
        let loaded_version = value
            .get("settings_version")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32;
        let mut settings = serde_json::from_value::<AppSettings>(value)
            .map_err(|error| format!("could not parse {}: {error}", self.path.display()))?;
        if loaded_version < 2 {
            settings.input_flip_x = false;
        }
        Ok(settings.sanitized())
    }

    pub fn load_or_default(&self) -> AppSettings {
        self.load().unwrap_or_default()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }

        let data = serde_json::to_string_pretty(&settings.sanitized())
            .map_err(|error| format!("could not serialize settings: {error}"))?;
        fs::write(&self.path, data)
            .map_err(|error| format!("could not write {}: {error}", self.path.display()))
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new()
    }
}
