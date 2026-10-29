// The dictation HUD on every OS, drawn by `rune_hud.rs` through the HUD
// overlay's Rune compositor: the capsule (`indicator_fragment`) and Dot scope
// (`dot_scope_fragment`). Positions are in points from the window's middle,
// y down; colours are sRGB-encoded, premultiplied.
//
// It began as a line-for-line port of the Metal HUD it replaced, and
// `rune_hud_frames` renders it against those frames. `smoothstep` is spelled
// out (`sstep`) because `coverage` relies on descending edges, which WGSL
// leaves to the backend.

struct Uniforms {
    resolution: vec2f,
    time: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    average: f32,
    peak: f32,
    processing: f32,
    post_processing: f32,
    capturing: f32,
    editing: f32,
    queued_count: f32,
    line_style: f32,
    line_count: f32,
    line_curvature: f32,
    line_speed: f32,
    line_sharpness: f32,
    line_glow: f32,
    sphere_depth: f32,
    light_angle: f32,
    sphere_outline: f32,
    completion: f32,
    recording_flash: f32,
    _padding: f32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn indicator_vertex(@builtin(vertex_index) vertex_id: u32) -> @builtin(position) vec4f {
    var positions = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    return vec4f(positions[vertex_id], 0.0, 1.0);
}

fn sstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn rounded_box(point: vec2f, half_size: vec2f, radius: f32) -> f32 {
    let q = abs(point) - half_size + radius;
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn coverage(distance: f32, softness: f32) -> f32 {
    return sstep(softness, -softness, distance);
}

fn glow(distance: f32, radius: f32) -> f32 {
    let outside = max(distance, 0.0);
    return exp2(-outside * outside / max(radius * radius, 0.001));
}

fn composite(destination: ptr<function, vec4f>, color: vec3f, alpha_in: f32) {
    let alpha = clamp(alpha_in, 0.0, 1.0);
    (*destination) = vec4f(
        color * alpha + (*destination).rgb * (1.0 - alpha),
        alpha + (*destination).a * (1.0 - alpha));
}

fn screen(destination: ptr<function, vec4f>, color: vec3f, amount_in: f32) {
    let amount = clamp(amount_in, 0.0, 1.0);
    (*destination) = vec4f(1.0 - (1.0 - (*destination).rgb) * (1.0 - color * amount), (*destination).a);
}

fn dithered(result_in: vec4f, position: vec2f) -> vec4f {
    var result = result_in;
    if (result.a > 0.0) {
        let pixel = floor(position);
        let n1 = fract(52.9829189 * fract(dot(pixel, vec2f(0.06711056, 0.00583715))));
        let n2 = fract(
            52.9829189 * fract(dot(pixel + vec2f(17.0, 31.0), vec2f(0.06711056, 0.00583715))));
        let dither = ((n1 - n2) / 255.0) * result.a;
        result = vec4f(clamp(result.rgb + vec3f(dither), vec3f(0.0), vec3f(result.a)), result.a);
    }
    return result;
}

@fragment
fn indicator_fragment(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / uniforms.resolution;
    let backing_scale = uniforms.resolution.x / 112.0;
    let point = (uv * uniforms.resolution - uniforms.resolution * 0.5)
        / backing_scale / max(uniforms.scale, 0.001);
    let half_size = vec2f(uniforms.width, uniforms.height) * 0.5;
    let radius = uniforms.height * 0.5;
    let distance = rounded_box(point, half_size, radius);
    let lifecycle_softness = max(uniforms.softness, 0.0);
    let edge = (max(fwidth(distance), 0.32) + lifecycle_softness * 0.72)
        / max(uniforms.scale, 0.001);
    let shape = coverage(distance, edge);
    let detail_clarity = exp2(-lifecycle_softness * 0.34);
    var result = vec4f(0.0);

    let processing = clamp(uniforms.processing, 0.0, 1.0);
    let post_processing = clamp(uniforms.post_processing, 0.0, 1.0);
    let editing = clamp(uniforms.editing, 0.0, 1.0);
    let completion = clamp(uniforms.completion, 0.0, 1.0);
    let recording_flash = clamp(uniforms.recording_flash, 0.0, 1.0)
        * (1.0 - processing);
    let completion_flash = sstep(0.0, 0.16, completion)
        * (1.0 - sstep(0.32, 1.0, completion));
    let recording = 1.0 - processing;
    let circularity = 1.0 - sstep(0.5, 2.25, abs(uniforms.width - uniforms.height));

    let red_accent = mix(
        vec3f(1.0, 0.025, 0.035),
        vec3f(0.05, 0.85, 0.58),
        editing);
    let blue_accent = vec3f(0.1, 0.34, 1.0);
    let violet_accent = vec3f(0.64, 0.2, 1.0);
    let pipeline_accent = mix(blue_accent, violet_accent, post_processing);
    let accent = mix(red_accent, pipeline_accent, processing);

    let average_power = clamp(uniforms.average, 0.0, 1.0);
    let peak_power = clamp(uniforms.peak, 0.0, 1.0);
    let average_activation = sstep(0.0, 0.1, average_power);
    var outer = recording * (
        glow(distance, 4.0 + lifecycle_softness * 0.5) * average_power * 0.72
        + glow(distance, 8.0 + lifecycle_softness) * average_power * 0.36);
    outer += processing * glow(distance, 2.6 + lifecycle_softness) * 0.045;
    outer += glow(distance, 6.0 + lifecycle_softness * 0.5) * completion_flash * 0.22;
    outer += glow(distance, 7.0 + lifecycle_softness * 0.5) * recording_flash * 0.5;
    composite(&result, accent, outer * (1.0 - shape));

    var recording_base = mix(
        vec3f(0.5, 0.0, 0.0),
        vec3f(1.0, 0.0, 0.0),
        average_power);
    recording_base = mix(recording_base, vec3f(0.0, 0.48, 0.3), editing);
    recording_base = mix(recording_base, vec3f(1.0, 0.08, 0.06), recording_flash * 0.72);
    let sphere_light = clamp(0.48 - point.x * 0.035 - point.y * 0.045, 0.0, 1.0);
    let blue_base = mix(
        vec3f(0.0, 0.02, 0.32),
        vec3f(0.04, 0.18, 0.68),
        sphere_light);
    let violet_base = mix(
        vec3f(0.12, 0.0, 0.3),
        vec3f(0.38, 0.04, 0.68),
        sphere_light);
    let pipeline_base = mix(blue_base, violet_base, post_processing);
    let base = mix(recording_base, pipeline_base, processing);
    composite(&result, base, shape);

    let inside_edge = clamp(-distance / 4.0, 0.0, 1.0);
    let inner_edge = shape * (1.0 - inside_edge);
    let inner_edge_strength = mix(0.48, 0.07, processing);
    composite(&result, accent, inner_edge * inner_edge_strength);

    // Match the original Hex recording stack: a padded red fill, a nearly
    // full-width white beam, and a broad peak-driven red glow.
    let beam_point = point - vec2f(0.0, -0.5);
    let red_fill_distance = rounded_box(beam_point, vec2f(22.0, 2.0), 2.0);
    let red_fill = glow(red_fill_distance, 2.0) * shape;
    screen(&result, red_accent, red_fill * average_activation * recording * detail_clarity);

    let white_beam_distance = rounded_box(beam_point, vec2f(21.0, 1.0), 1.0);
    let white_beam = glow(white_beam_distance, 1.0) * shape;
    screen(
        &result,
        vec3f(1.0),
        white_beam * average_activation * 0.56 * recording * detail_clarity);

    let peak_half_width = 22.0 * min(peak_power + 0.6, 1.0);
    let peak_distance = rounded_box(beam_point, vec2f(peak_half_width, 2.0), 2.0);
    let peak_beam = glow(peak_distance, 4.0) * shape;
    screen(
        &result,
        red_accent,
        peak_beam * sstep(0.0, 0.1, peak_power) * 0.5 * recording * detail_clarity);

    // Derivatives need uniform control flow, so these are taken before the
    // branches that use them.
    let orb_radius = uniforms.height * 0.5 - 0.6;
    let orb_distance = length(point) - orb_radius;
    let orb_edge = max(fwidth(orb_distance), 0.26);
    let curvature = mix(0.0, 3.0, clamp(uniforms.line_curvature, 0.0, 1.0));
    let sphere_uv = point / max(orb_radius, 0.001);
    let sweep_coordinate = point.x + sphere_uv.y * sphere_uv.y * curvature;
    let sweep_coordinate_width = fwidth(sweep_coordinate);
    if (processing > 0.0) {
        let orb_mask = coverage(orb_distance, orb_edge) * circularity;
        let surface_z = sqrt(max(1.0 - dot(sphere_uv, sphere_uv), 0.0));
        let depth = mix(0.55, 1.45, clamp(uniforms.sphere_depth, 0.0, 1.0));
        let normal_z = pow(surface_z, depth);
        let light_phase = uniforms.light_angle * 6.2831853
            + uniforms.time * uniforms.line_speed * 0.22;
        let light_direction = vec2f(cos(light_phase), sin(light_phase));
        let light = clamp(
            0.32 + dot(sphere_uv, light_direction) * 0.18 + normal_z * 0.28,
            0.0,
            1.0);
        let orb_shadow = mix(vec3f(0.0, 0.015, 0.24), vec3f(0.1, 0.0, 0.26), post_processing);
        let orb_light = mix(vec3f(0.05, 0.22, 0.72), vec3f(0.4, 0.05, 0.7), post_processing);
        composite(&result, mix(orb_shadow, orb_light, light), orb_mask * processing * detail_clarity);

        let rim = pow(1.0 - normal_z, 2.4);
        composite(&result, pipeline_accent, rim * orb_mask * processing * 0.3 * detail_clarity);

        let style = clamp(uniforms.line_style, 0.0, 2.0);
        let sharpness = clamp(uniforms.line_sharpness, 0.0, 1.0);
        let blur_amount = 1.0 - sharpness;
        let highlight_count = clamp(round(uniforms.line_count), 1.0, 6.0);
        var shine = 0.0;
        var soft_shine = 0.0;
        if (style < 0.5) {
            // A moving area light gives the orb motion without drawing stripes
            // across its face.
            let focus = clamp(
                dot(sphere_uv, light_direction) * 0.55 + normal_z * 0.72,
                0.0,
                1.0);
            shine = pow(focus, mix(3.0, 12.0, sharpness));
        } else if (style < 1.5) {
            // Meridian highlights wrap around the sphere instead of crossing
            // it as flat screen-space lines.
            let longitude = atan2(sphere_uv.x, normal_z) / 6.2831853;
            let travel = uniforms.time * uniforms.line_speed * 0.11;
            for (var index = 0; index < 6; index++) {
                if (f32(index) >= highlight_count) {
                    break;
                }
                let offset = f32(index) / highlight_count;
                let angular_delta = fract(longitude - travel - offset + 0.5) - 0.5;
                let angular_distance = abs(angular_delta);
                let core = exp2(-pow(angular_distance / 0.018, 2.0));
                let halo_radius = mix(0.025, 0.14, blur_amount);
                let halo = exp2(-pow(angular_distance / halo_radius, 2.0));
                shine = max(shine, core);
                soft_shine = max(soft_shine, halo);
            }
        } else {
            // Curved ribbons remain available in the lab for comparison, but
            // are no longer the production default.
            let sweep_travel = orb_radius * 2.0 + 18.0;
            let sweep_start = (uniforms.time * 38.0 * uniforms.line_speed) % sweep_travel
                - orb_radius - 7.0;
            let spacing = mix(7.0, 2.8, (highlight_count - 1.0) / 5.0);
            for (var index = 0; index < 6; index++) {
                if (f32(index) >= highlight_count) {
                    break;
                }
                let line_distance = abs(
                    sweep_coordinate - (sweep_start - f32(index) * spacing));
                // fwidth(|c - k|) is fwidth(c) wherever it is differentiable.
                let aa = max(sweep_coordinate_width, 0.24);
                let band = 1.0 - sstep(
                    mix(1.4, 0.25, sharpness) - aa,
                    mix(4.8, 1.5, sharpness) + aa,
                    line_distance);
                shine = max(shine, band);
            }
        }
        let shine_color = mix(vec3f(0.66, 0.84, 1.0), vec3f(0.92, 0.72, 1.0), post_processing);
        let shine_strength = mix(0.16, 0.68, clamp(uniforms.line_glow, 0.0, 1.0));
        let surface_wrap = orb_mask * mix(0.28, 1.0, normal_z);
        let rim_spill = (1.0 - orb_mask) * glow(orb_distance, 2.2);
        composite(
            &result,
            shine_color,
            soft_shine * (surface_wrap + rim_spill * 0.72) * processing * blur_amount
                * shine_strength * 0.52
                * detail_clarity);
        composite(
            &result,
            shine_color,
            shine * surface_wrap * processing * shine_strength
                * mix(1.0, 0.36, blur_amount) * detail_clarity);
    }

    // Pending jobs sit beside the foreground state. Keeping this geometry
    // stationary prevents queue status from reading as part of the sphere.
    let queued_count = min(uniforms.queued_count, 4.0);
    for (var index = 0; index < 4; index++) {
        if (f32(index) >= queued_count) {
            break;
        }
        let center = vec2f(
            uniforms.width * 0.5 + 10.0 + f32(index) * 3.4,
            0.0);
        let dot_distance = length(point - center) - 0.68;
        let dot_coverage = coverage(dot_distance, 0.22);
        var leading_processing = 0.0;
        if (index == 0) {
            leading_processing = post_processing * clamp(uniforms.capturing, 0.0, 1.0);
        }
        let queued_blue = vec3f(0.12, 0.58, 1.0);
        let dot_color = mix(queued_blue, violet_accent, leading_processing);
        composite(&result, dot_color, dot_coverage * select(0.52, 0.92, index == 0));
    }

    screen(&result, vec3f(0.72, 0.88, 1.0), shape * completion_flash * 0.32);
    let completion_rim = exp2(-pow(abs(distance) / 0.72, 2.0));
    screen(&result, vec3f(0.82, 0.93, 1.0), completion_rim * completion_flash * 0.58);

    let sphere_outline = clamp(uniforms.sphere_outline, 0.0, 1.0);
    let processing_stroke = mix(0.04, 0.4, sphere_outline);
    let stroke = coverage(
        abs(distance) - 0.4,
        0.38 + lifecycle_softness * 0.3) * mix(0.56, processing_stroke, processing) * detail_clarity;
    composite(&result, mix(accent, vec3f(1.0), 0.1), stroke);
    screen(&result, vec3f(1.0, 0.2, 0.16), shape * recording_flash * 0.36);
    let recording_flash_rim = exp2(-pow(abs(distance) / 0.9, 2.0));
    screen(&result, vec3f(1.0, 0.58, 0.48), recording_flash_rim * recording_flash * 0.82);

    return dithered(result * uniforms.opacity, position.xy);
}

// Dot scope (experiment `dot-scope`): a plate of dots lit by `dot_scope.rs`.
// Each dot's (brightness, freshness) arrives as normalized u16s, row by row;
// the plate, glow, and dot shapes follow the t422 prototype's look A.
struct DotUniforms {
    resolution: vec2f,
    backing_scale: f32,
    width: f32,
    height: f32,
    opacity: f32,
    scale: f32,
    softness: f32,
    pitch: f32,
    columns: f32,
    rows: f32,
    glow: f32,
    grid: f32,
    live: f32,
    cool: f32,
    dark: f32,
    base: vec4f,
    core: vec4f,
    plate: vec4f,
    rim: vec4f,
    shadow: vec4f,
    // The plate's middle from the window's (pt, y down), its fit, and 1 in
    // an island.
    plate_layout: vec4f,
    // The island's half width, height, top edge, and bottom radius, pt.
    island: vec4f,
}

// The island grown out of the notch: a box hanging from the screen's top
// edge, rounded below, with concave fillets where it meets the top edge.
fn island_distance(point: vec2f, island: vec4f) -> f32 {
    let half_width = island.x;
    let height = island.y;
    let top = island.z;
    let radius = max(island.w, 0.0);
    let fillet = clamp(height * 0.25, 0.0, 6.0);
    // Reaches `radius` above the top so only the lower corners round.
    let body_center = vec2f(0.0, top + (height - radius) * 0.5);
    let body = rounded_box(point - body_center, vec2f(half_width, (height + radius) * 0.5), radius);
    let q = vec2f(abs(point.x), point.y);
    let square_center = vec2f(half_width + fillet * 0.5, top + fillet * 0.5);
    let square = rounded_box(q - square_center, vec2f(fillet * 0.5 + 0.5, fillet * 0.5), 0.0);
    let hollow = fillet - length(q - vec2f(half_width + fillet, top + fillet));
    return min(body, max(square, hollow));
}

@group(0) @binding(1) var<uniform> u: DotUniforms;
// `MAX_DOTS` (960) dots, two u16 halves to a u32, four u32 to a vec4.
@group(0) @binding(2) var<uniform> dots: array<vec4u, 240>;

fn dot_value(index: i32) -> vec2f {
    let word = dots[index / 4][index % 4];
    return vec2f(f32(word & 0xffffu), f32(word >> 16u)) / 65535.0;
}

@fragment
fn dot_scope_fragment(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let uv = position.xy / u.resolution;
    let scale = max(u.scale, 0.001) * max(u.plate_layout.z, 0.01);
    let window_point = (uv * u.resolution - u.resolution * 0.5) / u.backing_scale;
    let point = (window_point - u.plate_layout.xy) / scale;
    let pixel = 1.0 / (u.backing_scale * scale);
    let softness = max(u.softness, 0.0);
    let aa = pixel * 0.75 + softness * 0.72;
    let half_size = vec2f(u.width, u.height) * 0.5;
    let radius = u.height * 0.5;
    let distance = rounded_box(point, half_size, radius);
    let dark = u.dark > 0.5;
    var result = vec4f(0.0);
    let in_island = u.plate_layout.w > 0.5;
    var island_shape = 1.0;
    if (in_island) {
        let island_aa = 0.75 / u.backing_scale;
        island_shape = coverage(island_distance(window_point, u.island), island_aa);
    }

    // A soft drop shadow 3 pt below, as the prototype's 16 pt canvas blur.
    let shadow_distance = rounded_box(point - vec2f(0.0, 3.0), half_size, radius);
    composite(&result, u.shadow.rgb, u.shadow.a * (0.5 - 0.5 * tanh(shadow_distance / 6.5)));

    let shape = coverage(distance, aa) * island_shape;
    composite(&result, u.plate.rgb, u.plate.a * shape);
    composite(&result, u.rim.rgb, u.rim.a * coverage(abs(distance) - 0.3, aa));

    if (u.glow > 0.01) {
        let reach = 34.0 + 20.0 * u.glow;
        let t = clamp(length(point) / reach, 0.0, 1.0);
        let strength = select(0.19, 0.3, dark) * 0.55 * min(1.0, u.glow);
        let middle = mix(u.base.rgb, u.core.rgb, 0.5);
        composite(&result, mix(middle, u.base.rgb, t), strength * (1.0 - t) * shape);
    }

    let columns = i32(u.columns + 0.5);
    let rows = i32(u.rows + 0.5);
    let pitch = u.pitch;
    let center_offset = vec2f(f32(columns) - 1.0, f32(rows) - 1.0) * 0.5;
    let i0 = i32(round(point.x / pitch + center_offset.x));
    let j0 = i32(round(point.y / pitch + center_offset.y));
    let unlit = select(0.085, 0.07, dark);
    let full = select(1.0, 0.96, dark);
    let curve = select(0.75, 0.9, dark);
    let presence = 0.5 + 0.5 * max(u.live, u.cool);
    for (var dj = -1; dj <= 1; dj++) {
        for (var di = -1; di <= 1; di++) {
            let i = i0 + di;
            let j = j0 + dj;
            if (i < 0 || j < 0 || i >= columns || j >= rows) {
                continue;
            }
            let center = (vec2f(f32(i), f32(j)) - center_offset) * pitch;
            let cap_dx = max(abs(center.x) - (u.width * 0.5 - radius), 0.0);
            let cap = radius - length(vec2f(cap_dx, center.y));
            let mask = sstep(1.4, 4.6, cap);
            if (mask <= 0.0) {
                continue;
            }
            let value = dot_value(j * columns + i);
            let power = max(value.x, 1e-5);
            let fresh = value.y;
            let row = abs(center.y) / radius;
            let floor_alpha = unlit * (1.0 - 0.7 * sstep(0.35, 0.95, row)) * presence * u.grid;
            let alpha = floor_alpha + (full - floor_alpha) * pow(power, curve * mix(1.4, 1.0, fresh));
            let dot_radius = (0.25 + 0.17 * pow(power, 0.6))
                * mix(1.0, mix(0.82, 1.0, fresh), sstep(0.03, 0.2, power)) * pitch;
            let core = clamp(power * power * 1.1, 0.0, 0.9) * mix(0.3, 1.0, fresh);
            let color = mix(u.base.rgb, u.core.rgb, core);
            let from_center = length(point - center);
            if (power > 0.05) {
                composite(
                    &result,
                    color,
                    0.132 * power * power * mask * shape * coverage(from_center - dot_radius * 2.0, aa));
            }
            composite(&result, color, alpha * mask * shape * coverage(from_center - dot_radius, aa));
        }
    }

    result *= u.opacity;
    // The island is part of the notch, solid black however faded the
    // plate's contents are.
    if (in_island) {
        result.a += island_shape * (1.0 - result.a);
    }
    return dithered(result, position.xy);
}
