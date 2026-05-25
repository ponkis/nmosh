use std::{borrow::Cow, time::Instant};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window};

use crate::{
    app::{AppSettings, MidiControl, MIDI_CONTROL_COUNT},
    midi::MidiSnapshot,
    ndi::NdiFrame,
    ui::{Overlay, OverlayRenderContext, UiAction, UiStatus},
};

const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const VIDEO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const GRID_SIZE: u32 = 96;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    scene_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    uniform: Uniforms,
    uniform_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    video_texture: TextureBundle,
    video_size: (u32, u32),
    latest_frame: Option<NdiFrame>,
    feedback: [TextureBundle; 2],
    depth: TextureBundle,
    scene_bind_groups: [wgpu::BindGroup; 2],
    present_bind_groups: [wgpu::BindGroup; 2],
    feedback_read: usize,
    start_time: Instant,
    last_frame_time: Instant,
    smooth: SmoothState,
    overlay: Overlay,
}

impl Renderer {
    pub async fn new(
        window: &'static Window,
        initial_ndi_filter: Option<&str>,
        initial_midi_filter: Option<&str>,
        initial_settings: AppSettings,
    ) -> Result<Self, String> {
        let size = nonzero_size(window.inner_size());
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| format!("failed to create GPU surface: {error}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| "no compatible GPU adapter found".to_string())?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|error| format!("failed to create GPU device: {error}"))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let present_mode = surface_caps
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Mailbox)
            .unwrap_or(wgpu::PresentMode::Fifo);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/effects.wgsl"))),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d video distortion pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_mesh",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_scene",
                targets: &[Some(wgpu::ColorTargetState {
                    format: OFFSCREEN_FORMAT,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_fullscreen",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_present",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let (vertices, indices) = build_mesh(GRID_SIZE);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("video mesh vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("video mesh indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let uniform = Uniforms::new(size.width, size.height, 2, 2);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effect uniforms"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let video_texture = create_video_texture(&device, 2, 2);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &video_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &initial_video_pixels(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(8),
                rows_per_image: Some(2),
            },
            wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
        );

        let feedback = [
            create_color_target(&device, size.width, size.height, "feedback 0"),
            create_color_target(&device, size.width, size.height, "feedback 1"),
        ];
        let depth = create_depth_target(&device, size.width, size.height);

        let scene_bind_groups = [
            create_bind_group(
                &device,
                &bind_group_layout,
                &uniform_buffer,
                &video_texture.view,
                &feedback[0].view,
                &sampler,
                "scene bind group 0",
            ),
            create_bind_group(
                &device,
                &bind_group_layout,
                &uniform_buffer,
                &video_texture.view,
                &feedback[1].view,
                &sampler,
                "scene bind group 1",
            ),
        ];
        let present_bind_groups = [
            create_bind_group(
                &device,
                &bind_group_layout,
                &uniform_buffer,
                &feedback[0].view,
                &feedback[0].view,
                &sampler,
                "present bind group 0",
            ),
            create_bind_group(
                &device,
                &bind_group_layout,
                &uniform_buffer,
                &feedback[1].view,
                &feedback[1].view,
                &sampler,
                "present bind group 1",
            ),
        ];
        let overlay = Overlay::new(
            window,
            &device,
            surface_format,
            initial_ndi_filter,
            initial_midi_filter,
            initial_settings,
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            scene_pipeline,
            present_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            uniform,
            uniform_buffer,
            bind_group_layout,
            sampler,
            video_texture,
            video_size: (2, 2),
            latest_frame: None,
            feedback,
            depth,
            scene_bind_groups,
            present_bind_groups,
            feedback_read: 0,
            start_time: Instant::now(),
            last_frame_time: Instant::now(),
            smooth: SmoothState::default(),
            overlay,
        })
    }

    pub fn handle_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.overlay.handle_window_event(window, event)
    }

    pub fn toggle_options(&mut self) {
        self.overlay.toggle();
    }

    pub fn close_options(&mut self) {
        self.overlay.close();
    }

    pub fn options_open(&self) -> bool {
        self.overlay.is_open()
    }

    pub fn reset_effects(&mut self) {
        self.smooth = SmoothState::default();
        self.feedback_read = 0;
    }

    pub fn sample_ndi_color_at(
        &self,
        position_points: [f32; 2],
        scale_factor: f32,
        settings: &AppSettings,
    ) -> Option<[f32; 3]> {
        let frame = self.latest_frame.as_ref()?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }

        let scale_factor = scale_factor.max(0.5);
        let viewport_width = self.size.width as f32 / scale_factor;
        let viewport_height = self.size.height as f32 / scale_factor;
        if viewport_width <= 0.0 || viewport_height <= 0.0 {
            return None;
        }

        let screen_x = (position_points[0] / viewport_width).clamp(0.0, 1.0);
        let screen_y = (position_points[1] / viewport_height).clamp(0.0, 1.0);
        let ndc_x = screen_x * 2.0 - 1.0;
        let ndc_y = 1.0 - screen_y * 2.0;

        let window_aspect = (self.size.width as f32 / self.size.height.max(1) as f32).max(0.1);
        let source_aspect = (frame.width as f32 / frame.height.max(1) as f32).max(0.1);
        let target_aspect = if settings.aspect_mode.as_uniform() > 0.5 {
            4.0 / 3.0
        } else {
            source_aspect
        };
        let fit = if target_aspect > window_aspect {
            [1.0, window_aspect / target_aspect]
        } else {
            [target_aspect / window_aspect, 1.0]
        };

        let zoom = zoom_factor(settings.zoom);
        let local_x = ndc_x / (fit[0] * zoom).max(0.001);
        let local_y = ndc_y / (fit[1] * zoom).max(0.001);
        if !(-1.0..=1.0).contains(&local_x) || !(-1.0..=1.0).contains(&local_y) {
            return None;
        }

        let mut uv = [(local_x + 1.0) * 0.5, (1.0 - local_y) * 0.5];
        uv[1] = 1.0 - uv[1];
        if settings.input_flip_x {
            uv[0] = 1.0 - uv[0];
        }
        if settings.input_flip_y {
            uv[1] = 1.0 - uv[1];
        }

        let x = (uv[0].clamp(0.0, 1.0) * (frame.width - 1) as f32).round() as usize;
        let y = (uv[1].clamp(0.0, 1.0) * (frame.height - 1) as f32).round() as usize;
        let offset = (y * frame.width as usize + x) * 4;
        let pixel = frame.data.get(offset..offset + 4)?;

        Some([
            pixel[2] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[0] as f32 / 255.0,
        ])
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        let size = nonzero_size(size);
        if size == self.size {
            return;
        }

        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);

        self.feedback = [
            create_color_target(&self.device, size.width, size.height, "feedback 0"),
            create_color_target(&self.device, size.width, size.height, "feedback 1"),
        ];
        self.depth = create_depth_target(&self.device, size.width, size.height);
        self.feedback_read = 0;
        self.rebuild_bind_groups();
    }

    pub fn upload_ndi_frame(&mut self, frame: &NdiFrame) {
        if frame.width == 0 || frame.height == 0 || frame.data.is_empty() {
            return;
        }

        let expected_len = frame.width as usize * frame.height as usize * 4;
        if frame.data.len() < expected_len {
            return;
        }

        self.latest_frame = Some(frame.clone());

        if self.video_size != (frame.width, frame.height) {
            self.video_texture = create_video_texture(&self.device, frame.width, frame.height);
            self.video_size = (frame.width, frame.height);
            self.rebuild_bind_groups();
        }

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.video_texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.data[..expected_len],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn render(
        &mut self,
        window: &Window,
        midi: MidiSnapshot,
        settings: &mut AppSettings,
        status: UiStatus<'_>,
    ) -> Result<Vec<UiAction>, wgpu::SurfaceError> {
        self.update_uniforms(midi, settings);
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.uniform));

        let frame = self.surface.get_current_texture()?;
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let write_index = 1 - self.feedback_read;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.feedback[write_index].view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind_groups[self.feedback_read], &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &self.present_bind_groups[write_index], &[]);
            pass.draw(0..3, 0..1);
        }

        let actions = self.overlay.render(
            OverlayRenderContext {
                window,
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                target: &surface_view,
                screen_size: [self.size.width, self.size.height],
            },
            settings,
            midi,
            status,
        );

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.feedback_read = write_index;

        Ok(actions)
    }

    fn update_uniforms(&mut self, midi: MidiSnapshot, settings: &AppSettings) {
        let now = Instant::now();
        let dt = (now - self.last_frame_time).as_secs_f32().clamp(0.0, 0.05);
        self.last_frame_time = now;

        let energy_follow = response(dt, 12.0);
        let slow_follow = response(dt, 5.0);
        self.smooth.energy = lerp(self.smooth.energy, midi.note_energy, energy_follow);
        self.smooth.gate = lerp(self.smooth.gate, midi.gate, energy_follow);
        self.smooth.pitch = lerp(self.smooth.pitch, midi.pitch, slow_follow);
        self.smooth.bend = lerp(self.smooth.bend, midi.bend, energy_follow);
        self.smooth.aftertouch = lerp(self.smooth.aftertouch, midi.aftertouch, energy_follow);

        if midi.trigger_count != self.smooth.last_trigger_count {
            self.smooth.shock = 1.0;
            self.smooth.last_trigger_count = midi.trigger_count;
        } else {
            self.smooth.shock *= (-dt * 3.0).exp();
        }

        let cc = midi.cc;
        let mappings = settings.midi_bindings;
        let target_controls = [
            (mappings.binding(MidiControl::Warp).value_from(&cc) * 1.1
                + self.smooth.energy * 0.55
                + self.smooth.aftertouch * 0.35)
                .min(1.0),
            (mappings.binding(MidiControl::Chroma).value_from(&cc) + self.smooth.shock * 0.35)
                .min(1.0),
            1.0 + mappings.binding(MidiControl::Brightness).value_from(&cc) * 0.45
                + self.smooth.energy * 0.10,
            mappings.binding(MidiControl::Hue).value_from(&cc),
            (mappings.binding(MidiControl::Feedback).value_from(&cc) * 0.92
                + self.smooth.energy * 0.04)
                .min(0.96),
            (mappings.binding(MidiControl::Glitch).value_from(&cc) + self.smooth.shock * 0.75)
                .min(1.0),
            mappings.binding(MidiControl::Scanlines).value_from(&cc),
            mappings.binding(MidiControl::Kaleidoscope).value_from(&cc),
            (mappings.binding(MidiControl::Depth).value_from(&cc) * 1.35
                + self.smooth.energy * 0.32)
                .min(1.8),
            mappings.binding(MidiControl::Rotation).value_from(&cc),
            mappings.binding(MidiControl::Pixelate).value_from(&cc),
            mappings.binding(MidiControl::Edge).value_from(&cc),
            mappings.binding(MidiControl::Tunnel).value_from(&cc),
            mappings.binding(MidiControl::Invert).value_from(&cc),
            mappings.binding(MidiControl::Zoom).value_from(&cc),
            mappings.binding(MidiControl::Cube).value_from(&cc),
            mappings
                .binding(MidiControl::Flash)
                .value_from_sources(&cc, &midi.notes),
            mappings
                .binding(MidiControl::ChromaTolerance)
                .value_from(&cc),
            mappings
                .binding(MidiControl::ChromaSoftness)
                .value_from(&cc),
            mappings.binding(MidiControl::Posterize).value_from(&cc),
            mappings.binding(MidiControl::Thermal).value_from(&cc),
            mappings.binding(MidiControl::Spare).value_from(&cc),
        ];

        for (value, target) in self.smooth.controls.iter_mut().zip(target_controls) {
            *value = lerp(*value, target, slow_follow);
        }

        self.uniform.time_params = [
            self.start_time.elapsed().as_secs_f32(),
            dt,
            self.smooth.energy,
            self.smooth.pitch,
        ];
        self.uniform.midi_params = [
            self.smooth.gate,
            self.smooth.bend,
            self.smooth.shock,
            self.smooth.aftertouch,
        ];
        self.uniform.resolution = [
            self.size.width as f32,
            self.size.height as f32,
            self.video_size.0 as f32,
            self.video_size.1 as f32,
        ];
        self.uniform
            .controls0
            .copy_from_slice(&self.smooth.controls[0..4]);
        self.uniform
            .controls1
            .copy_from_slice(&self.smooth.controls[4..8]);
        self.uniform
            .controls2
            .copy_from_slice(&self.smooth.controls[8..12]);
        self.uniform
            .controls3
            .copy_from_slice(&self.smooth.controls[12..16]);
        self.uniform.app_params = [
            settings.camera_mode.as_uniform(),
            if settings.input_flip_x { 1.0 } else { 0.0 },
            if settings.input_flip_y { 1.0 } else { 0.0 },
            settings.aspect_mode.as_uniform(),
        ];
        self.uniform.view_params = [
            zoom_factor(settings.zoom + self.smooth.controls[MidiControl::Zoom as usize] * 24.0),
            (settings
                .cube_amount
                .max(self.smooth.controls[MidiControl::Cube as usize])
                .max(if settings.inside_box { 1.0 } else { 0.0 }))
            .clamp(0.0, 1.0),
            0.0,
            settings
                .posterize_amount
                .max(self.smooth.controls[MidiControl::Posterize as usize])
                .clamp(0.0, 1.0),
        ];
        self.uniform.chroma_key = [
            settings.chroma_key.color[0],
            settings.chroma_key.color[1],
            settings.chroma_key.color[2],
            if settings.chroma_key.enabled {
                1.0
            } else {
                0.0
            },
        ];
        self.uniform.chroma_params = [
            (settings.chroma_key.tolerance
                + self.smooth.controls[MidiControl::ChromaTolerance as usize] * 0.5)
                .clamp(0.0, 1.0),
            (settings.chroma_key.softness
                + self.smooth.controls[MidiControl::ChromaSoftness as usize] * 0.5)
                .clamp(0.001, 1.0),
            settings.chroma_key.spill,
            settings
                .thermal_amount
                .max(self.smooth.controls[MidiControl::Thermal as usize])
                .clamp(0.0, 1.0),
        ];
        self.uniform.effect_params = [
            settings
                .tunnel_amount
                .max(self.smooth.controls[MidiControl::Tunnel as usize])
                .clamp(0.0, 1.0),
            if settings.inside_box { 1.0 } else { 0.0 },
            mappings
                .binding(MidiControl::Flash)
                .value_from_sources(&cc, &midi.notes)
                .clamp(0.0, 1.0),
            0.0,
        ];
    }

    fn rebuild_bind_groups(&mut self) {
        self.scene_bind_groups = [
            create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.video_texture.view,
                &self.feedback[0].view,
                &self.sampler,
                "scene bind group 0",
            ),
            create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.video_texture.view,
                &self.feedback[1].view,
                &self.sampler,
                "scene bind group 1",
            ),
        ];
        self.present_bind_groups = [
            create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.feedback[0].view,
                &self.feedback[0].view,
                &self.sampler,
                "present bind group 0",
            ),
            create_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.feedback[1].view,
                &self.feedback[1].view,
                &self.sampler,
                "present bind group 1",
            ),
        ];
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    time_params: [f32; 4],
    midi_params: [f32; 4],
    resolution: [f32; 4],
    controls0: [f32; 4],
    controls1: [f32; 4],
    controls2: [f32; 4],
    controls3: [f32; 4],
    app_params: [f32; 4],
    view_params: [f32; 4],
    chroma_key: [f32; 4],
    chroma_params: [f32; 4],
    effect_params: [f32; 4],
}

impl Uniforms {
    fn new(width: u32, height: u32, video_width: u32, video_height: u32) -> Self {
        Self {
            time_params: [0.0, 0.0, 0.0, 0.5],
            midi_params: [0.0, 0.0, 0.0, 0.0],
            resolution: [
                width as f32,
                height as f32,
                video_width as f32,
                video_height as f32,
            ],
            controls0: [0.0, 0.0, 1.0, 0.0],
            controls1: [0.0, 0.0, 0.0, 0.0],
            controls2: [0.3, 0.0, 0.0, 0.0],
            controls3: [0.0, 0.0, 0.0, 0.0],
            app_params: [0.0, 0.0, 0.0, 0.0],
            view_params: [1.0, 0.0, 0.0, 0.0],
            chroma_key: [0.0, 1.0, 0.0, 0.0],
            chroma_params: [0.22, 0.12, 0.35, 0.0],
            effect_params: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

#[derive(Default)]
struct SmoothState {
    energy: f32,
    gate: f32,
    pitch: f32,
    bend: f32,
    shock: f32,
    aftertouch: f32,
    controls: [f32; MIDI_CONTROL_COUNT],
    last_trigger_count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    plane_position: [f32; 3],
    cube_position: [f32; 3],
    uv: [f32; 2],
    face: f32,
}

impl Vertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

struct TextureBundle {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

fn build_mesh(size: u32) -> (Vec<Vertex>, Vec<u32>) {
    let vertex_count = (size + 1) * (size + 1) * 6;
    let index_count = size * size * 6 * 6;
    let mut vertices = Vec::with_capacity(vertex_count as usize);
    let mut indices = Vec::with_capacity(index_count as usize);

    for face in 0..6_u32 {
        let base = vertices.len() as u32;
        for y in 0..=size {
            for x in 0..=size {
                let u = x as f32 / size as f32;
                let v = y as f32 / size as f32;
                let px = u * 2.0 - 1.0;
                let py = 1.0 - v * 2.0;
                vertices.push(Vertex {
                    plane_position: collapsed_plane_position(face, px, py),
                    cube_position: cube_position(face, px, py),
                    uv: [u, v],
                    face: face as f32,
                });
            }
        }

        let row = size + 1;
        for y in 0..size {
            for x in 0..size {
                let i = base + y * row + x;
                indices.extend_from_slice(&[i, i + 1, i + row, i + 1, i + row + 1, i + row]);
            }
        }
    }

    (vertices, indices)
}

fn collapsed_plane_position(face: u32, x: f32, y: f32) -> [f32; 3] {
    match face {
        0 => [x, y, 0.0],
        1 => [0.0, 0.0, 0.0],
        2 => [-1.0, y, 0.0],
        3 => [1.0, y, 0.0],
        4 => [x, 1.0, 0.0],
        _ => [x, -1.0, 0.0],
    }
}

fn cube_position(face: u32, x: f32, y: f32) -> [f32; 3] {
    match face {
        0 => [x, y, 1.0],
        1 => [-x, y, -1.0],
        2 => [-1.0, y, -x],
        3 => [1.0, y, x],
        4 => [x, 1.0, -y],
        _ => [x, -1.0, y],
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    source_view: &wgpu::TextureView,
    feedback_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(feedback_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_video_texture(device: &wgpu::Device, width: u32, height: u32) -> TextureBundle {
    create_texture(
        device,
        width,
        height,
        VIDEO_FORMAT,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        "ndi video texture",
    )
}

fn create_color_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> TextureBundle {
    create_texture(
        device,
        width,
        height,
        OFFSCREEN_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        label,
    )
}

fn create_depth_target(device: &wgpu::Device, width: u32, height: u32) -> TextureBundle {
    create_texture(
        device,
        width,
        height,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
        "depth target",
    )
}

fn create_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    label: &str,
) -> TextureBundle {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    TextureBundle { texture, view }
}

fn initial_video_pixels() -> [u8; 16] {
    [0; 16]
}

fn nonzero_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn response(dt: f32, speed: f32) -> f32 {
    1.0 - (-dt * speed).exp()
}

fn lerp(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount.clamp(0.0, 1.0)
}

fn zoom_factor(zoom: f32) -> f32 {
    2.0_f32.powf(zoom.clamp(-64.0, 64.0))
}
