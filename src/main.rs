mod app;
mod midi;
mod ndi;
mod renderer;
mod ui;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use app::{AppSettings, CameraMode, SettingsStore, APP_NAME, APP_VERSION};
use midi::MidiController;
use ndi::NdiInput;
use renderer::Renderer;
use ui::{UiAction, UiStatus};
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
    keyboard::{Key, NamedKey},
    window::{Fullscreen, Window, WindowBuilder},
};

fn main() {
    env_logger::init();

    let options = match Options::from_env() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return;
        }
    };

    if options.help {
        print_usage();
        return;
    }

    let initial_ndi_name = options.ndi_name.clone();
    let initial_midi_name = options.midi_name.clone();
    let ndi_dll = options.ndi_dll.clone();
    let settings_store = SettingsStore::new();
    let mut settings = settings_store.load_or_default();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let icon_bytes = include_bytes!("icon_rgba.bin");
    let icon = winit::window::Icon::from_rgba(icon_bytes.to_vec(), 64, 64).ok();

    let mut window_builder = WindowBuilder::new()
        .with_title(format!("{APP_NAME} v{APP_VERSION}"))
        .with_inner_size(LogicalSize::new(options.width, options.height))
        .with_min_inner_size(LogicalSize::new(640_u32, 360_u32));

    if let Some(icon) = icon {
        window_builder = window_builder.with_window_icon(Some(icon));
    }

    let window = window_builder
        .build(&event_loop)
        .expect("failed to create native window");

    let window: &'static Window = Box::leak(Box::new(window));
    let mut renderer = match pollster::block_on(Renderer::new(
        window,
        initial_ndi_name.as_deref(),
        initial_midi_name.as_deref(),
        settings,
    )) {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("GPU initialization failed: {error}");
            return;
        }
    };

    let mut midi = MidiController::open(initial_midi_name.as_deref());
    log::info!("MIDI: {}", midi.status());

    let mut ndi = NdiInput::start(initial_ndi_name, ndi_dll.clone());
    let mut last_title_update = Instant::now() - Duration::from_secs(5);
    let mut last_reset_event_counter = 0_u64;

    let _ = event_loop.run(move |event, event_loop| {
        event_loop.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => {
                let consumed_by_ui = renderer.handle_window_event(window, &event);

                match event {
                    WindowEvent::CloseRequested => event_loop.exit(),
                    WindowEvent::Resized(size) => renderer.resize(size),
                    WindowEvent::KeyboardInput { event, .. } if !consumed_by_ui => {
                        handle_keyboard(window, event_loop, &mut renderer, &mut settings, event);
                    }
                    WindowEvent::RedrawRequested => {
                        if let Some(frame) = ndi.take_latest_frame() {
                            renderer.upload_ndi_frame(&frame);
                        }

                        let mut midi_snapshot = midi.snapshot();
                        if should_reset_effects(
                            &settings,
                            midi_snapshot,
                            &mut last_reset_event_counter,
                        ) {
                            midi.reset_effect_values(&settings.midi_bindings);
                            settings.reset_effects_to_clean();
                            renderer.reset_effects();
                            midi_snapshot = midi.snapshot();
                        }

                        let midi_status = midi.status().to_string();
                        let ndi_status = ndi.status();
                        match renderer.render(
                            window,
                            midi_snapshot,
                            &mut settings,
                            UiStatus {
                                midi: &midi_status,
                                ndi: &ndi_status,
                            },
                        ) {
                            Ok(actions) => {
                                let mut action_context = UiActionContext {
                                    midi: &mut midi,
                                    ndi: &mut ndi,
                                    ndi_dll: &ndi_dll,
                                    settings_store: &settings_store,
                                    renderer: &mut renderer,
                                    settings: &mut settings,
                                    window_scale_factor: window.scale_factor() as f32,
                                };
                                for action in actions {
                                    handle_ui_action(action, &mut action_context);
                                }
                            }
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                renderer.resize(window.inner_size());
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                            Err(wgpu::SurfaceError::Timeout) => {}
                        }

                        if last_title_update.elapsed() >= Duration::from_millis(750) {
                            window.set_title(&format!(
                                "{APP_NAME} v{APP_VERSION} | MIDI: {} | NDI: {}",
                                midi.status(),
                                ndi.status()
                            ));
                            last_title_update = Instant::now();
                        }
                    }
                    _ => {}
                }
            }
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    });
}

fn handle_keyboard(
    window: &Window,
    event_loop: &EventLoopWindowTarget<()>,
    renderer: &mut Renderer,
    settings: &mut AppSettings,
    event: KeyEvent,
) {
    if event.state != ElementState::Pressed || event.repeat {
        return;
    }

    match &event.logical_key {
        Key::Named(NamedKey::F11) => toggle_fullscreen(window),
        Key::Named(NamedKey::Escape) => {
            if renderer.options_open() {
                renderer.close_options();
            } else if window.fullscreen().is_some() {
                window.set_fullscreen(None);
            } else {
                event_loop.exit();
            }
        }
        Key::Character(character) if character.eq_ignore_ascii_case("o") => {
            renderer.toggle_options();
        }
        Key::Character(character) if character.eq_ignore_ascii_case("c") => {
            settings.camera_mode = settings.camera_mode.toggled();
        }
        Key::Character(character) if character == "1" => {
            settings.camera_mode = CameraMode::Free;
        }
        Key::Character(character) if character == "2" => {
            settings.camera_mode = CameraMode::FixedPlane;
        }
        Key::Character(character) if character.eq_ignore_ascii_case("f") => {
            toggle_fullscreen(window);
        }
        _ => {}
    }
}

struct UiActionContext<'a> {
    midi: &'a mut MidiController,
    ndi: &'a mut NdiInput,
    ndi_dll: &'a Option<PathBuf>,
    settings_store: &'a SettingsStore,
    renderer: &'a mut Renderer,
    settings: &'a mut AppSettings,
    window_scale_factor: f32,
}

fn handle_ui_action(action: UiAction, context: &mut UiActionContext<'_>) {
    match action {
        UiAction::ReconnectMidi(name) => {
            *context.midi = MidiController::open(name.as_deref());
        }
        UiAction::ReconnectNdi(name) => {
            *context.ndi = NdiInput::start(name, context.ndi_dll.clone());
        }
        UiAction::ResetEffects => {
            context
                .midi
                .reset_effect_values(&context.settings.midi_bindings);
            context.settings.reset_effects_to_clean();
            context.renderer.reset_effects();
        }
        UiAction::SaveSettings(settings) => {
            if let Err(error) = context.settings_store.save(&settings) {
                log::error!("{error}");
            } else {
                log::info!(
                    "Saved settings to {}",
                    context.settings_store.path().display()
                );
            }
        }
        UiAction::SampleChromaAt(position) => {
            if let Some(color) = context.renderer.sample_ndi_color_at(
                position,
                context.window_scale_factor,
                context.settings,
            ) {
                context.settings.chroma_key.color = color;
                context.settings.chroma_key.enabled = true;
            }
        }
    }
}

fn should_reset_effects(
    settings: &AppSettings,
    midi: midi::MidiSnapshot,
    last_reset_event_counter: &mut u64,
) -> bool {
    let Some(event) = midi.last_event else {
        return false;
    };
    if event.counter == *last_reset_event_counter || event.value < 0.05 {
        return false;
    }
    if !settings.midi_bindings.reset_effects.matches(event) {
        return false;
    }

    *last_reset_event_counter = event.counter;
    true
}

fn toggle_fullscreen(window: &Window) {
    if window.fullscreen().is_some() {
        window.set_fullscreen(None);
    } else {
        window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
    }
}

#[derive(Debug)]
struct Options {
    ndi_name: Option<String>,
    ndi_dll: Option<PathBuf>,
    midi_name: Option<String>,
    width: u32,
    height: u32,
    help: bool,
}

impl Options {
    fn from_env() -> Result<Self, String> {
        let mut options = Self {
            ndi_name: None,
            ndi_dll: None,
            midi_name: None,
            width: 1280,
            height: 720,
            help: false,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => options.help = true,
                "--ndi" => {
                    options.ndi_name = Some(
                        args.next()
                            .ok_or_else(|| "--ndi requires a source-name substring".to_string())?,
                    );
                }
                "--ndi-dll" => {
                    options.ndi_dll =
                        Some(PathBuf::from(args.next().ok_or_else(|| {
                            "--ndi-dll requires a DLL/library path".to_string()
                        })?));
                }
                "--midi" => {
                    options.midi_name = Some(
                        args.next()
                            .ok_or_else(|| "--midi requires a port-name substring".to_string())?,
                    );
                }
                "--width" => {
                    options.width = parse_dimension("--width", args.next())?;
                }
                "--height" => {
                    options.height = parse_dimension("--height", args.next())?;
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }

        Ok(options)
    }
}

fn parse_dimension(name: &str, value: Option<String>) -> Result<u32, String> {
    let value = value.ok_or_else(|| format!("{name} requires a positive integer"))?;
    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn print_usage() {
    eprintln!(
        "Usage: nmosh [--ndi SOURCE_SUBSTRING] [--ndi-dll PATH] [--midi PORT_SUBSTRING] [--width PX] [--height PX]"
    );
}
