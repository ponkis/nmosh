use std::sync::{Arc, Mutex};

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::app::{MidiBindings, MidiEvent, MidiSource};

pub struct MidiController {
    shared: Arc<Mutex<RawMidiState>>,
    status: String,
    _connection: Option<MidiInputConnection<()>>,
}

#[derive(Clone, Copy, Debug)]
pub struct MidiSnapshot {
    pub cc: [f32; 128],
    pub notes: [f32; 128],
    pub last_event: Option<MidiEvent>,
    pub note_energy: f32,
    pub pitch: f32,
    pub gate: f32,
    pub aftertouch: f32,
    pub bend: f32,
    pub trigger_count: u64,
}

impl Default for MidiSnapshot {
    fn default() -> Self {
        Self {
            cc: [0.0; 128],
            notes: [0.0; 128],
            last_event: None,
            note_energy: 0.0,
            pitch: 0.5,
            gate: 0.0,
            aftertouch: 0.0,
            bend: 0.0,
            trigger_count: 0,
        }
    }
}

impl MidiController {
    pub fn available_ports() -> Vec<String> {
        let midi_in = match MidiInput::new("nMosh MIDI scan") {
            Ok(input) => input,
            Err(error) => return vec![format!("MIDI unavailable ({error})")],
        };

        midi_in
            .ports()
            .iter()
            .map(|port| {
                midi_in
                    .port_name(port)
                    .unwrap_or_else(|_| "unknown".to_string())
            })
            .collect()
    }

    pub fn open(preferred_name: Option<&str>) -> Self {
        let shared = Arc::new(Mutex::new(RawMidiState::default()));
        let mut midi_in = match MidiInput::new("nMosh") {
            Ok(input) => input,
            Err(error) => {
                return Self {
                    shared,
                    status: format!("unavailable ({error})"),
                    _connection: None,
                };
            }
        };

        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();
        if ports.is_empty() {
            return Self {
                shared,
                status: "no MIDI ports".to_string(),
                _connection: None,
            };
        }

        let names = ports
            .iter()
            .map(|port| {
                midi_in
                    .port_name(port)
                    .unwrap_or_else(|_| "unknown".to_string())
            })
            .collect::<Vec<_>>();

        let selected_index = preferred_name
            .and_then(|preferred| {
                names
                    .iter()
                    .position(|name| contains_case_insensitive(name, preferred))
            })
            .unwrap_or(0);

        let selected_port = ports[selected_index].clone();
        let selected_name = names[selected_index].clone();
        let callback_state = Arc::clone(&shared);

        let connection = match midi_in.connect(
            &selected_port,
            "nMosh MIDI input",
            move |_timestamp, message, _| {
                if let Ok(mut state) = callback_state.lock() {
                    state.apply_message(message);
                }
            },
            (),
        ) {
            Ok(connection) => Some(connection),
            Err(error) => {
                return Self {
                    shared,
                    status: format!("failed to open MIDI port '{selected_name}' ({error})"),
                    _connection: None,
                };
            }
        };

        Self {
            shared,
            status: selected_name,
            _connection: connection,
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn snapshot(&self) -> MidiSnapshot {
        let Ok(state) = self.shared.lock() else {
            return MidiSnapshot::default();
        };

        let mut weighted_pitch = 0.0;
        let mut total_velocity = 0.0;
        let mut max_velocity = 0_u8;
        let mut active_notes = 0_u32;

        for (note, velocity) in state.notes.iter().enumerate() {
            if *velocity == 0 {
                continue;
            }

            let velocity = *velocity as f32 / 127.0;
            weighted_pitch += note as f32 * velocity;
            total_velocity += velocity;
            max_velocity = max_velocity.max(state.notes[note]);
            active_notes += 1;
        }

        let pitch = if total_velocity > 0.0 {
            weighted_pitch / total_velocity / 127.0
        } else {
            state.last_pitch as f32 / 127.0
        };

        let mut notes = [0.0; 128];
        for (index, velocity) in state.notes.iter().copied().enumerate() {
            notes[index] = velocity as f32 / 127.0;
        }

        MidiSnapshot {
            cc: state.cc,
            notes,
            last_event: state.last_event,
            note_energy: (max_velocity as f32 / 127.0).max((total_velocity / 6.0).min(1.0)),
            pitch,
            gate: if active_notes > 0 { 1.0 } else { 0.0 },
            aftertouch: state.aftertouch,
            bend: state.pitch_bend,
            trigger_count: state.trigger_count,
        }
    }

    pub fn reset_effect_values(&self, bindings: &MidiBindings) {
        let Ok(mut state) = self.shared.lock() else {
            return;
        };

        for binding in bindings.bindings().iter().copied() {
            if binding.source == MidiSource::ControlChange {
                state.cc[binding.number.min(127) as usize] = 0.0;
            }
        }

        state.notes = [0; 128];
        state.aftertouch = 0.0;
        state.pitch_bend = 0.0;
        state.trigger_count = state.trigger_count.wrapping_add(1);
    }
}

#[derive(Clone)]
struct RawMidiState {
    cc: [f32; 128],
    notes: [u8; 128],
    aftertouch: f32,
    pitch_bend: f32,
    last_pitch: u8,
    event_counter: u64,
    last_event: Option<MidiEvent>,
    trigger_count: u64,
}

impl Default for RawMidiState {
    fn default() -> Self {
        Self {
            cc: [0.0; 128],
            notes: [0; 128],
            aftertouch: 0.0,
            pitch_bend: 0.0,
            last_pitch: 64,
            event_counter: 0,
            last_event: None,
            trigger_count: 0,
        }
    }
}

impl RawMidiState {
    fn apply_message(&mut self, message: &[u8]) {
        if message.is_empty() {
            return;
        }

        let status = message[0] & 0xF0;
        match status {
            0x80 if message.len() >= 2 => {
                self.notes[message[1].min(127) as usize] = 0;
            }
            0x90 if message.len() >= 3 => {
                let note = message[1].min(127) as usize;
                let velocity = message[2].min(127);
                if velocity == 0 {
                    self.notes[note] = 0;
                } else {
                    self.notes[note] = velocity;
                    self.last_pitch = note as u8;
                    self.trigger_count = self.trigger_count.wrapping_add(1);
                    self.record_event(MidiSource::Note, note as u8, velocity as f32 / 127.0);
                }
            }
            0xA0 if message.len() >= 3 => {
                self.aftertouch = message[2].min(127) as f32 / 127.0;
            }
            0xB0 if message.len() >= 3 => {
                let controller = message[1].min(127) as usize;
                let value = message[2].min(127) as f32 / 127.0;
                self.cc[controller] = value;
                self.record_event(MidiSource::ControlChange, controller as u8, value);
            }
            0xD0 if message.len() >= 2 => {
                self.aftertouch = message[1].min(127) as f32 / 127.0;
            }
            0xE0 if message.len() >= 3 => {
                let lsb = message[1].min(127) as i32;
                let msb = message[2].min(127) as i32;
                let value = (msb << 7) | lsb;
                self.pitch_bend = ((value as f32 - 8192.0) / 8192.0).clamp(-1.0, 1.0);
            }
            _ => {}
        }
    }

    fn record_event(&mut self, source: MidiSource, number: u8, value: f32) {
        self.event_counter = self.event_counter.wrapping_add(1);
        self.last_event = Some(MidiEvent {
            source,
            number,
            value,
            counter: self.event_counter,
        });
    }
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
