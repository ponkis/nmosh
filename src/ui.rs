use std::time::{Duration, Instant};

use egui::{Align, Color32, RichText, Vec2};
use egui_wgpu::ScreenDescriptor;
use winit::{event::WindowEvent, window::Window};

use crate::{
    app::{
        AppSettings, AspectMode, CameraMode, MidiBinding, MidiControl, MidiSource, APP_CREDIT,
        APP_NAME, APP_VERSION, MIDI_CONTROLS, MIDI_CONTROL_LABELS,
    },
    midi::{MidiController, MidiSnapshot},
};

pub struct Overlay {
    open: bool,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    ndi_filter: String,
    selected_midi: String,
    midi_ports: Vec<String>,
    last_port_refresh: Instant,
    saved_settings: AppSettings,
    dirty: bool,
    save_prompt_open: bool,
    learn_target: Option<LearnTarget>,
    learn_started_after: u64,
    eyedropper_active: bool,
    page: UiPage,
    viewport_points: [f32; 2],
}

pub struct UiStatus<'a> {
    pub midi: &'a str,
    pub ndi: &'a str,
}

pub struct OverlayRenderContext<'a> {
    pub window: &'a Window,
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub target: &'a wgpu::TextureView,
    pub screen_size: [u32; 2],
}

#[derive(Debug)]
pub enum UiAction {
    ReconnectNdi(Option<String>),
    ReconnectMidi(Option<String>),
    ResetEffects,
    SaveSettings(AppSettings),
    SampleChromaAt([f32; 2]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LearnTarget {
    Control(MidiControl),
    ResetEffects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiPage {
    Inputs,
    View,
    Key,
    Effects,
    Midi,
}

impl Overlay {
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        initial_ndi_filter: Option<&str>,
        initial_midi_filter: Option<&str>,
        initial_settings: AppSettings,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
        );
        let renderer = egui_wgpu::Renderer::new(device, surface_format, None, 1);

        Self {
            open: false,
            egui_ctx,
            egui_state,
            renderer,
            ndi_filter: initial_ndi_filter.unwrap_or_default().to_string(),
            selected_midi: initial_midi_filter.unwrap_or_default().to_string(),
            midi_ports: MidiController::available_ports(),
            last_port_refresh: Instant::now(),
            saved_settings: initial_settings,
            dirty: false,
            save_prompt_open: false,
            learn_target: None,
            learn_started_after: 0,
            eyedropper_active: false,
            page: UiPage::Inputs,
            viewport_points: [640.0, 360.0],
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.request_close();
        } else {
            self.open = true;
            self.refresh_midi_ports();
        }
    }

    pub fn close(&mut self) {
        if self.open {
            self.request_close();
        }
    }

    pub fn handle_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        if !self.open {
            return false;
        }

        self.egui_state.on_window_event(window, event).consumed
    }

    pub fn render(
        &mut self,
        render_context: OverlayRenderContext<'_>,
        settings: &mut AppSettings,
        midi: MidiSnapshot,
        status: UiStatus<'_>,
    ) -> Vec<UiAction> {
        if !self.open {
            return Vec::new();
        }

        if self.last_port_refresh.elapsed() >= Duration::from_secs(2) {
            self.refresh_midi_ports();
        }

        let points_per_pixel = 1.0 / self.egui_ctx.pixels_per_point().max(0.5);
        self.viewport_points = [
            render_context.screen_size[0] as f32 * points_per_pixel,
            render_context.screen_size[1] as f32 * points_per_pixel,
        ];

        self.consume_learn_event(settings, midi);

        let raw_input = self.egui_state.take_egui_input(render_context.window);
        let mut actions = Vec::new();
        let egui_ctx = self.egui_ctx.clone();
        let full_output = egui_ctx.run(raw_input, |ctx| {
            self.consume_eyedropper_click(ctx, &mut actions);
            self.draw(ctx, settings, midi, status, &mut actions);
        });

        self.egui_state
            .handle_platform_output(render_context.window, full_output.platform_output);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(
                render_context.device,
                render_context.queue,
                *id,
                image_delta,
            );
        }

        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, self.egui_ctx.pixels_per_point());
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: render_context.screen_size,
            pixels_per_point: self.egui_ctx.pixels_per_point(),
        };

        self.renderer.update_buffers(
            render_context.device,
            render_context.queue,
            render_context.encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let mut render_pass =
                render_context
                    .encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("egui overlay pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: render_context.target,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
            self.renderer
                .render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        actions
    }

    fn draw(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        midi: MidiSnapshot,
        status: UiStatus<'_>,
        actions: &mut Vec<UiAction>,
    ) {
        *settings = settings.sanitized();
        self.dirty = *settings != self.saved_settings;

        let panel_width = self.viewport_points[0].clamp(360.0, 540.0);
        let panel_height = (self.viewport_points[1] - 24.0).clamp(300.0, 640.0);
        let content_height = (panel_height - 142.0).max(160.0);

        let frame = egui::Frame::window(&ctx.style())
            .fill(Color32::from_rgba_unmultiplied(18, 20, 23, 242));

        egui::Window::new(format!("{APP_NAME} v{APP_VERSION}"))
            .frame(frame)
            .fixed_size(Vec2::new(panel_width, panel_height))
            .anchor(egui::Align2::LEFT_TOP, Vec2::new(12.0, 12.0))
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                self.draw_header(ui, settings, actions);
                ui.separator();
                self.draw_status_row(ui, status);
                self.draw_tabs(ui);
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_source("options-page-scroll")
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .show(ui, |ui| match self.page {
                        UiPage::Inputs => self.draw_inputs(ui, actions),
                        UiPage::View => self.draw_view_settings(ui, settings),
                        UiPage::Key => self.draw_chroma_key(ui, settings),
                        UiPage::Effects => self.draw_effects(ui, settings, actions),
                        UiPage::Midi => self.draw_midi_learn(ui, settings, midi),
                    });
            });

        self.draw_save_prompt(ctx, settings, actions);
        self.dirty = *settings != self.saved_settings;
    }

    fn draw_header(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut AppSettings,
        actions: &mut Vec<UiAction>,
    ) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new(APP_NAME).strong());
                ui.label(APP_CREDIT);
            });
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.request_close();
                }
            });
        });

        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.dirty, egui::Button::new("Save"))
                .clicked()
            {
                self.save_current(settings, actions);
            }
            if ui.button("Reset defaults").clicked() {
                *settings = AppSettings::default();
                self.dirty = *settings != self.saved_settings;
            }
            if self.dirty {
                ui.colored_label(Color32::from_rgb(255, 204, 112), "Unsaved");
            } else {
                ui.colored_label(Color32::from_rgb(120, 220, 150), "Saved");
            }
        });
    }

    fn draw_status_row(&mut self, ui: &mut egui::Ui, status: UiStatus<'_>) {
        ui.small(format!("MIDI: {}", status.midi));
        ui.small(format!("NDI: {}", status.ndi));
    }

    fn draw_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            self.tab(ui, UiPage::Inputs, "Inputs");
            self.tab(ui, UiPage::View, "View");
            self.tab(ui, UiPage::Key, "Key");
            self.tab(ui, UiPage::Effects, "FX");
            self.tab(ui, UiPage::Midi, "MIDI");
        });
    }

    fn tab(&mut self, ui: &mut egui::Ui, page: UiPage, label: &str) {
        if ui.selectable_label(self.page == page, label).clicked() {
            self.page = page;
        }
    }

    fn draw_inputs(&mut self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        ui.heading("Inputs");
        ui.horizontal(|ui| {
            ui.label("NDI");
            ui.text_edit_singleline(&mut self.ndi_filter);
        });
        ui.horizontal(|ui| {
            if ui.button("Reconnect NDI").clicked() {
                actions.push(UiAction::ReconnectNdi(non_empty(self.ndi_filter.clone())));
            }
            if ui.button("First visible source").clicked() {
                self.ndi_filter.clear();
                actions.push(UiAction::ReconnectNdi(None));
            }
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("MIDI");
            egui::ComboBox::from_id_source("midi-input")
                .selected_text(if self.selected_midi.is_empty() {
                    "first available"
                } else {
                    self.selected_midi.as_str()
                })
                .width(280.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.selected_midi, String::new(), "first available");
                    for port in &self.midi_ports {
                        ui.selectable_value(&mut self.selected_midi, port.clone(), port);
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui.button("Reconnect MIDI").clicked() {
                actions.push(UiAction::ReconnectMidi(non_empty(
                    self.selected_midi.clone(),
                )));
            }
            if ui.button("Refresh MIDI").clicked() {
                self.refresh_midi_ports();
            }
        });
    }

    fn draw_view_settings(&mut self, ui: &mut egui::Ui, settings: &mut AppSettings) {
        ui.heading("View");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut settings.camera_mode, CameraMode::Free, "Free camera");
            ui.selectable_value(
                &mut settings.camera_mode,
                CameraMode::FixedPlane,
                "Fixed camera",
            );
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut settings.aspect_mode, AspectMode::Source, "Source");
            ui.selectable_value(&mut settings.aspect_mode, AspectMode::FourThree, "4:3");
        });
        ui.horizontal(|ui| {
            ui.label("Zoom");
            ui.add(egui::DragValue::new(&mut settings.zoom).speed(0.05));
        });
        ui.checkbox(&mut settings.input_flip_x, "Flip video horizontally");
        ui.checkbox(&mut settings.input_flip_y, "Flip video vertically");
    }

    fn draw_chroma_key(&mut self, ui: &mut egui::Ui, settings: &mut AppSettings) {
        ui.heading("Chroma key");
        ui.checkbox(&mut settings.chroma_key.enabled, "Enable chroma key");
        ui.horizontal(|ui| {
            ui.label("Key color");
            ui.color_edit_button_rgb(&mut settings.chroma_key.color);
            if ui.button("Eyedropper").clicked() {
                self.eyedropper_active = true;
            }
        });
        if self.eyedropper_active {
            ui.colored_label(
                Color32::from_rgb(111, 214, 255),
                "Click the video outside this panel to sample a key color.",
            );
            if ui.button("Cancel eyedropper").clicked() {
                self.eyedropper_active = false;
            }
        }
        ui.add(egui::Slider::new(&mut settings.chroma_key.tolerance, 0.0..=1.0).text("Tolerance"));
        ui.add(egui::Slider::new(&mut settings.chroma_key.softness, 0.001..=1.0).text("Softness"));
        ui.add(egui::Slider::new(&mut settings.chroma_key.spill, 0.0..=1.0).text("Spill"));
    }

    fn draw_effects(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut AppSettings,
        actions: &mut Vec<UiAction>,
    ) {
        ui.heading("Effects");
        ui.add(egui::Slider::new(&mut settings.cube_amount, 0.0..=1.0).text("Cube morph"));
        ui.checkbox(&mut settings.inside_box, "Inside box");
        ui.add(egui::Slider::new(&mut settings.tunnel_amount, 0.0..=1.0).text("Tunnel"));
        ui.add(egui::Slider::new(&mut settings.posterize_amount, 0.0..=1.0).text("Posterize"));
        ui.add(egui::Slider::new(&mut settings.thermal_amount, 0.0..=1.0).text("Thermal color"));
        if ui.button("Reset effects now").clicked() {
            actions.push(UiAction::ResetEffects);
        }
    }

    fn draw_midi_learn(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut AppSettings,
        midi: MidiSnapshot,
    ) {
        ui.heading("MIDI learn");
        if let Some(target) = self.learn_target {
            ui.colored_label(
                Color32::from_rgb(111, 214, 255),
                format!(
                    "Learning {}. Move a MIDI control.",
                    learn_target_label(target)
                ),
            );
            if ui.button("Cancel learn").clicked() {
                self.learn_target = None;
            }
        }

        egui::Grid::new("midi-learn-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for (index, control) in MIDI_CONTROLS.iter().copied().enumerate() {
                    ui.label(MIDI_CONTROL_LABELS[index]);
                    ui.label(settings.midi_bindings.binding(control).label());
                    if ui.button("Learn").clicked() {
                        self.start_learning(LearnTarget::Control(control), midi);
                    }
                    if ui.button("Clear").clicked() {
                        assign_unique_binding(
                            settings,
                            LearnTarget::Control(control),
                            MidiBinding::none(),
                        );
                    }
                    ui.end_row();
                }

                ui.label("Reset effects");
                ui.label(settings.midi_bindings.reset_effects.label());
                if ui.button("Learn").clicked() {
                    self.start_learning(LearnTarget::ResetEffects, midi);
                }
                if ui.button("Clear").clicked() {
                    assign_unique_binding(settings, LearnTarget::ResetEffects, MidiBinding::none());
                }
                ui.end_row();
            });
    }

    fn draw_save_prompt(
        &mut self,
        ctx: &egui::Context,
        settings: &mut AppSettings,
        actions: &mut Vec<UiAction>,
    ) {
        if !self.save_prompt_open {
            return;
        }

        egui::Window::new("Save changes?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Save the current nMosh settings before closing options?");
                ui.horizontal(|ui| {
                    if ui.button("Save & close").clicked() {
                        self.save_current(settings, actions);
                        self.open = false;
                        self.save_prompt_open = false;
                    }
                    if ui.button("Discard").clicked() {
                        *settings = self.saved_settings;
                        self.dirty = false;
                        self.open = false;
                        self.save_prompt_open = false;
                        self.learn_target = None;
                        self.eyedropper_active = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.save_prompt_open = false;
                    }
                });
            });
    }

    fn consume_learn_event(&mut self, settings: &mut AppSettings, midi: MidiSnapshot) {
        let Some(target) = self.learn_target else {
            return;
        };
        let Some(event) = midi.last_event else {
            return;
        };
        if event.counter <= self.learn_started_after || event.source == MidiSource::None {
            return;
        }

        match target {
            LearnTarget::Control(control)
                if event.source != MidiSource::ControlChange && control != MidiControl::Flash => {}
            _ => {
                assign_unique_binding(settings, target, event.as_binding());
                self.learn_target = None;
            }
        }
    }

    fn consume_eyedropper_click(&mut self, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
        if !self.eyedropper_active || ctx.is_pointer_over_area() {
            return;
        }

        let clicked = ctx.input(|input| input.pointer.primary_clicked());
        if !clicked {
            return;
        }

        if let Some(position) = ctx.input(|input| input.pointer.latest_pos()) {
            actions.push(UiAction::SampleChromaAt([position.x, position.y]));
            self.eyedropper_active = false;
        }
    }

    fn start_learning(&mut self, target: LearnTarget, midi: MidiSnapshot) {
        self.learn_target = Some(target);
        self.learn_started_after = midi.last_event.map(|event| event.counter).unwrap_or(0);
    }

    fn request_close(&mut self) {
        if self.dirty {
            self.save_prompt_open = true;
        } else {
            self.open = false;
            self.learn_target = None;
            self.eyedropper_active = false;
        }
    }

    fn save_current(&mut self, settings: &mut AppSettings, actions: &mut Vec<UiAction>) {
        *settings = settings.sanitized();
        self.saved_settings = *settings;
        self.dirty = false;
        actions.push(UiAction::SaveSettings(*settings));
    }

    fn refresh_midi_ports(&mut self) {
        self.midi_ports = MidiController::available_ports();
        self.last_port_refresh = Instant::now();
    }
}

fn assign_unique_binding(settings: &mut AppSettings, target: LearnTarget, binding: MidiBinding) {
    if binding.source != MidiSource::None {
        for control in MIDI_CONTROLS {
            if settings.midi_bindings.binding(control) == binding {
                settings
                    .midi_bindings
                    .set_binding(control, MidiBinding::none());
            }
        }
        if settings.midi_bindings.reset_effects == binding {
            settings.midi_bindings.reset_effects = MidiBinding::none();
        }
    }

    match target {
        LearnTarget::Control(control) => settings.midi_bindings.set_binding(control, binding),
        LearnTarget::ResetEffects => settings.midi_bindings.reset_effects = binding,
    }
}

fn learn_target_label(target: LearnTarget) -> &'static str {
    match target {
        LearnTarget::Control(control) => MIDI_CONTROL_LABELS[control as usize],
        LearnTarget::ResetEffects => "Reset effects",
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
