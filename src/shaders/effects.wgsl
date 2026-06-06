struct Uniforms {
    time_params: vec4<f32>,
    midi_params: vec4<f32>,
    resolution: vec4<f32>,
    controls0: vec4<f32>,
    controls1: vec4<f32>,
    controls2: vec4<f32>,
    controls3: vec4<f32>,
    app_params: vec4<f32>,
    view_params: vec4<f32>,
    chroma_key: vec4<f32>,
    chroma_params: vec4<f32>,
    effect_params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var source_tex: texture_2d<f32>;
@group(0) @binding(2) var feedback_tex: texture_2d<f32>;
@group(0) @binding(3) var source_sampler: sampler;

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) cube_position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) face: f32,
};

struct MeshOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world: vec3<f32>,
    @location(2) face: f32,
};

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn saturate3(v: vec3<f32>) -> vec3<f32> {
    return clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn safe_uv(uv: vec2<f32>) -> vec2<f32> {
    return clamp(uv, vec2<f32>(0.001), vec2<f32>(0.999));
}

fn oriented_uv(uv_in: vec2<f32>) -> vec2<f32> {
    var uv = vec2<f32>(uv_in.x, 1.0 - uv_in.y);
    if (u.app_params.y > 0.5) {
        uv.x = 1.0 - uv.x;
    }
    if (u.app_params.z > 0.5) {
        uv.y = 1.0 - uv.y;
    }
    return uv;
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn rotate_x(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn rotate_y(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(p.x * c + p.z * s, p.y, -p.x * s + p.z * c);
}

fn hue_rotate(color: vec3<f32>, amount: f32) -> vec3<f32> {
    let angle = amount * 6.28318530718;
    let s = sin(angle);
    let c = cos(angle);
    let weights = vec3<f32>(0.299, 0.587, 0.114);

    let r = vec3<f32>(
        weights.x + (1.0 - weights.x) * c - weights.x * s,
        weights.y - weights.y * c - weights.y * s,
        weights.z - weights.z * c + (1.0 - weights.z) * s
    );
    let g = vec3<f32>(
        weights.x - weights.x * c + 0.143 * s,
        weights.y + (1.0 - weights.y) * c + 0.140 * s,
        weights.z - weights.z * c - 0.283 * s
    );
    let b = vec3<f32>(
        weights.x - weights.x * c - (1.0 - weights.x) * s,
        weights.y - weights.y * c + weights.y * s,
        weights.z + (1.0 - weights.z) * c + weights.z * s
    );

    return saturate3(vec3<f32>(dot(color, r), dot(color, g), dot(color, b)));
}



fn aspect_fit_scale() -> vec2<f32> {
    let window_aspect = max(u.resolution.x / max(u.resolution.y, 1.0), 0.1);
    let video_aspect = max(u.resolution.z / max(u.resolution.w, 1.0), 0.1);
    let target_aspect = select(video_aspect, 4.0 / 3.0, u.app_params.w > 0.5);

    if (target_aspect > window_aspect) {
        return vec2<f32>(1.0, window_aspect / target_aspect);
    }
    return vec2<f32>(target_aspect / window_aspect, 1.0);
}

fn kaleidoscope(uv: vec2<f32>, amount: f32) -> vec2<f32> {
    let blend = smoothstep(0.0, 0.75, amount);
    if (blend < 0.0001) {
        return uv;
    }

    let pi = 3.14159265359;
    let segments = mix(3.0, 14.0, smoothstep(0.15, 1.0, amount));
    let centered = uv - vec2<f32>(0.5);
    let radius = length(centered);
    var angle = atan2(centered.y, centered.x);
    let sector = 2.0 * pi / segments;
    angle = abs((angle - floor(angle / sector) * sector) - sector * 0.5);
    let kalei_uv = vec2<f32>(0.5) + vec2<f32>(cos(angle), sin(angle)) * radius;
    return mix(uv, kalei_uv, blend);
}

fn tunnel(uv: vec2<f32>, amount: f32) -> vec2<f32> {
    let blend = smoothstep(0.0, 0.85, amount);
    if (blend < 0.0001) {
        return uv;
    }

    let pi = 3.14159265359;
    let centered = uv - vec2<f32>(0.5);
    let radius = max(length(centered), 0.002);
    var angle = atan2(centered.y, centered.x) / (2.0 * pi);
    angle = abs(fract(angle * mix(4.0, 12.0, amount)) - 0.5) * 2.0;

    let flow = u.time_params.x * mix(0.18, 1.15, amount);
    let rings = fract((0.22 / radius) + flow);
    let twist = sin(log(radius + 0.015) * 8.0 - flow * 10.0) * 0.08 * amount;
    let tunnel_uv = vec2<f32>(
        fract(angle + twist),
        rings
    );

    return mix(uv, tunnel_uv, blend);
}

fn room_projection(screen_uv: vec2<f32>) -> vec2<f32> {
    let aspect = max(u.resolution.x / max(u.resolution.y, 1.0), 0.2);
    let yaw = (u.controls2.y - 0.5) * 0.9 + u.midi_params.y * 0.35 + sin(u.time_params.x * 0.11) * 0.08;
    let pitch = (u.time_params.w - 0.5) * 0.28;
    var dir = normalize(vec3<f32>(
        (screen_uv.x * 2.0 - 1.0) * aspect,
        (1.0 - screen_uv.y * 2.0) * 0.78,
        -1.18
    ));
    dir = rotate_y(rotate_x(dir, pitch), yaw);

    let tx = 1.0 / max(abs(dir.x), 0.0001);
    let ty = 1.0 / max(abs(dir.y), 0.0001);
    let tz = 1.0 / max(abs(dir.z), 0.0001);
    let t_hit = min(tx, min(ty, tz));
    let hit = dir * t_hit;

    var wall_uv = vec2<f32>(hit.x * 0.5 + 0.5, hit.y * 0.5 + 0.5);
    if (tx < ty && tx < tz) {
        wall_uv = vec2<f32>(hit.z * 0.5 + 0.5, hit.y * 0.5 + 0.5);
    } else if (ty < tz) {
        wall_uv = vec2<f32>(hit.x * 0.5 + 0.5, hit.z * 0.5 + 0.5);
    }

    return safe_uv(wall_uv);
}

fn distorted_uv(uv_in: vec2<f32>) -> vec2<f32> {
    let t = u.time_params.x;
    let energy = u.time_params.z;
    let pitch = u.time_params.w;
    let gate = u.midi_params.x;
    let bend = u.midi_params.y;
    let shock = u.midi_params.z;
    let warp = u.controls0.x;
    let glitch = u.controls1.y;

    var uv = oriented_uv(uv_in);
    let center = uv - vec2<f32>(0.5);
    let radius = length(center);
    let angle = atan2(center.y, center.x);
    let swirl = (warp * 2.35 + shock * 1.4) * (1.0 - smoothstep(0.0, 0.78, radius));
    uv = vec2<f32>(0.5) + vec2<f32>(cos(angle + swirl), sin(angle + swirl)) * radius;

    uv.x += sin(uv.y * (8.0 + warp * 45.0) + t * (1.3 + energy * 8.0)) * (0.004 + warp * 0.035);
    uv.y += cos(uv.x * (5.0 + pitch * 18.0) - t * (1.1 + gate * 6.0)) * (warp * 0.024);
    uv += normalize(center + vec2<f32>(0.0001)) * sin(radius * 35.0 - t * (2.0 + energy * 13.0)) * shock * 0.035;

    let block_y = floor(uv.y * mix(10.0, 90.0, glitch));
    let block_noise = hash21(vec2<f32>(block_y, floor(t * 24.0)));
    let tear = step(0.72, block_noise) * glitch;
    uv.x += (block_noise - 0.5) * tear * (0.05 + shock * 0.09 + abs(bend) * 0.05);

    let pixel = u.controls2.z;
    if (pixel > 0.01) {
        let cells = mix(850.0, 58.0, pixel);
        uv = (floor(uv * cells) + vec2<f32>(0.5)) / cells;
    }

    uv = kaleidoscope(uv, u.controls1.w);
    uv = tunnel(uv, u.effect_params.x);
    return safe_uv(uv);
}

@vertex
fn vs_mesh(input: MeshIn) -> MeshOut {
    let t = u.time_params.x;
    let energy = u.time_params.z;
    let pitch = u.time_params.w;
    let bend = u.midi_params.y;
    let shock = u.midi_params.z;
    let warp = u.controls0.x;
    let depth_amount = u.controls2.x;
    let rotation = u.controls2.y;
    let aspect = max(u.resolution.x / max(u.resolution.y, 1.0), 0.2);
    let free_camera = 1.0 - step(0.5, u.app_params.x);
    let inside_box = step(0.5, u.effect_params.y);
    let cube_amount = smoothstep(0.0, 1.0, u.view_params.y);
    let fit = aspect_fit_scale();

    if (inside_box > 0.5) {
        var room_out: MeshOut;
        room_out.position = vec4<f32>(input.position.xy, 0.0, 1.0);
        room_out.uv = input.uv;
        room_out.world = vec3<f32>(input.position.xy, 0.0);
        room_out.face = input.face;
        return room_out;
    }

    let object = mix(input.position, input.cube_position, cube_amount);
    let flat_scale = vec3<f32>(
        2.15 * fit.x * u.view_params.x,
        1.28 * fit.y * u.view_params.x,
        0.0
    );
    let cube_scale = vec3<f32>(
        1.12 * u.view_params.x,
        1.12 * u.view_params.x,
        1.12 * u.view_params.x
    );
    var local = object * mix(flat_scale, cube_scale, cube_amount);

    let wave_a = sin(local.x * (2.8 + warp * 5.0) + t * (1.7 + energy * 5.0));
    let wave_b = cos(local.y * (3.6 + pitch * 8.0) - t * (1.2 + abs(bend) * 5.0));
    let ripple = sin(length(local.xy) * (7.0 + warp * 24.0) - t * (2.4 + energy * 12.0));

    local.z += (wave_a * 0.13 + wave_b * 0.09 + ripple * (0.11 + shock * 0.34)) * depth_amount;
    local.x += sin(local.y * 4.0 + t) * warp * 0.035;
    local.y += cos(local.x * 3.0 - t * 0.8) * warp * 0.025;

    local = rotate_x(local, ((rotation - 0.5) * 0.55 + bend * 0.16) * free_camera + cube_amount * 0.58);
    local = rotate_y(local, (sin(t * 0.23) * 0.08 + bend * 0.22 + (pitch - 0.5) * 0.22) * free_camera + cube_amount * (0.95 + rotation * 2.4 + t * 0.2));
    let p = local + vec3<f32>(0.0, 0.0, -3.15);

    let near = 0.05;
    let far = 20.0;
    let focal = 1.30;
    let depth = max(-p.z, near);
    let ndc_z = (depth - near) / (far - near);

    var out: MeshOut;
    out.position = vec4<f32>(p.x * focal / aspect, p.y * focal, ndc_z * depth, depth);
    out.uv = input.uv;
    out.world = p;
    out.face = input.face;
    return out;
}

@fragment
fn fs_scene(input: MeshOut) -> @location(0) vec4<f32> {
    if (u.effect_params.y > 0.5 && input.face > 0.5) {
        discard;
    }
    if (u.effect_params.y <= 0.5 && input.face > 0.5 && u.view_params.y < 0.015) {
        discard;
    }

    let t = u.time_params.x;
    let energy = u.time_params.z;
    let gate = u.midi_params.x;
    let shock = u.midi_params.z;
    var source_uv = input.uv;
    if (u.effect_params.y > 0.5) {
        source_uv = room_projection(input.uv);
    }

    let uv = distorted_uv(source_uv);
    let texel = vec2<f32>(1.0) / max(u.resolution.zw, vec2<f32>(1.0));

    let chroma = (u.controls0.y * 0.012 + shock * 0.010);
    let chroma_axis = normalize(vec2<f32>(
        cos(t * 0.77 + input.world.x),
        sin(t * 0.61 + input.world.y)
    ) + vec2<f32>(0.001));

    let r = textureSample(source_tex, source_sampler, safe_uv(uv + chroma_axis * chroma)).r;
    let raw_sample = textureSample(source_tex, source_sampler, uv);
    let raw_color = raw_sample.rgb;
    let g = raw_color.g;
    let b = textureSample(source_tex, source_sampler, safe_uv(uv - chroma_axis * chroma)).b;
    var color = vec3<f32>(r, g, b);
    var matte = raw_sample.a;
    let black_floor = 0.018;
    color = max(color - vec3<f32>(black_floor), vec3<f32>(0.0)) / (1.0 - black_floor);

    if (u.chroma_key.w > 0.5) {
        let distance_to_key = distance(raw_color, u.chroma_key.rgb);
        let key_matte = smoothstep(u.chroma_params.x, u.chroma_params.x + u.chroma_params.y, distance_to_key);
        matte *= key_matte;
        let spill_target = max(color.r, color.b);
        color.g = mix(color.g, min(color.g, spill_target), (1.0 - key_matte) * u.chroma_params.z);
    }

    let edge_amount = u.controls2.w;
    if (edge_amount > 0.01) {
        let left = textureSample(source_tex, source_sampler, safe_uv(uv - vec2<f32>(texel.x * 2.0, 0.0))).rgb;
        let right = textureSample(source_tex, source_sampler, safe_uv(uv + vec2<f32>(texel.x * 2.0, 0.0))).rgb;
        let up = textureSample(source_tex, source_sampler, safe_uv(uv + vec2<f32>(0.0, texel.y * 2.0))).rgb;
        let down = textureSample(source_tex, source_sampler, safe_uv(uv - vec2<f32>(0.0, texel.y * 2.0))).rgb;
        let edge = length(right - left) + length(up - down);
        color += vec3<f32>(edge) * edge_amount * (0.6 + energy);
    }

    color = hue_rotate(color, u.controls0.w);
    color *= u.controls0.z;



    let feedback_amount = u.controls1.x;
    if (feedback_amount > 0.001) {
        let center = input.uv - vec2<f32>(0.5);
        let feedback_uv = safe_uv(input.uv + center * (0.018 + feedback_amount * 0.035) + vec2<f32>(sin(t * 1.7), cos(t * 1.1)) * shock * 0.006);
        let feedback_sample = textureSample(feedback_tex, source_sampler, feedback_uv);
        let feedback = feedback_sample.rgb * feedback_sample.a;
        color = mix(color, feedback * (0.96 + energy * 0.16), feedback_amount);
    }

    let scanlines = u.controls1.z;
    if (scanlines > 0.001) {
        let scan = 0.5 + 0.5 * sin(input.uv.y * u.resolution.y * 3.14159265);
        let rgb_shift = vec3<f32>(1.0 + scanlines * 0.06, 1.0, 1.0 - scanlines * 0.05);
        color *= rgb_shift * (1.0 - scanlines * mix(0.03, 0.18, scan));
    }

    let invert = u.controls3.y;
    if (invert > 0.001) {
        let solar = 1.0 - abs(color * 2.0 - 1.0);
        color = mix(color, solar, invert * (0.5 + 0.5 * gate));
    }

    let grain_amount = u.controls1.y * 0.025 + shock * 0.035;
    if (grain_amount > 0.001) {
        let grain = (hash21(input.uv * u.resolution.xy + vec2<f32>(floor(t * 60.0))) - 0.5) * grain_amount;
        color += vec3<f32>(grain);
    }

    color *= matte;
    return vec4<f32>(saturate3(color), matte);
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> FullscreenOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0)
    );

    let p = positions[vertex_index];
    var out: FullscreenOut;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = p * 0.5 + vec2<f32>(0.5);
    return out;
}

@fragment
fn fs_present(input: FullscreenOut) -> @location(0) vec4<f32> {
    let sample = textureSample(source_tex, source_sampler, safe_uv(input.uv));
    var color = sample.rgb * sample.a;
    let flash = clamp(u.effect_params.z, 0.0, 1.0);
    if (flash > 0.001) {
        let strobe = step(0.5, fract(u.time_params.x * 18.0));
        color = mix(color, vec3<f32>(1.0), flash * strobe);
    }
    return vec4<f32>(saturate3(color), 1.0);
}
