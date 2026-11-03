// The dictation HUD on every OS, drawn by `rune_hud.rs` through the HUD
// overlay's Rune compositor: Dot scope, a plate of dots lit by
// `dot_scope.rs`. Positions are in points from the window's middle, y down;
// colours are sRGB-encoded, premultiplied. `smoothstep` is spelled out
// (`sstep`) because `coverage` relies on descending edges, which WGSL leaves
// to the backend.
//
// The viewport is the HUD's window: the whole overlay, or the Sounds sheet's
// stage in the Settings window. `uv` runs across it and `position` is the
// pixel within it. A linear (sRGB-format) target gets the colours decoded.

override linear_target: bool = false;

struct Varyings {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn indicator_vertex(@builtin(vertex_index) vertex_id: u32) -> Varyings {
    var positions = array<vec2f, 3>(vec2f(-1.0, -1.0), vec2f(3.0, -1.0), vec2f(-1.0, 3.0));
    let p = positions[vertex_id];
    return Varyings(vec4f(p, 0.0, 1.0), vec2f(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5));
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

fn composite(destination: ptr<function, vec4f>, color: vec3f, alpha_in: f32) {
    let alpha = clamp(alpha_in, 0.0, 1.0);
    (*destination) = vec4f(
        color * alpha + (*destination).rgb * (1.0 - alpha),
        alpha + (*destination).a * (1.0 - alpha));
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

// Premultiplied display colour as the target takes it.
fn for_target(result: vec4f) -> vec4f {
    if (!linear_target || result.a <= 0.0) {
        return result;
    }
    let c = result.rgb / result.a;
    let linear = select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
    return vec4f(linear * result.a, result.a);
}

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
    // The field's corner radius, pt: half its height on a plate, the
    // island's corners less its padding in an island. With the notch
    // indicator's ears (y 1): the notch's half width, the mode, the voice's
    // 0–1 level, and seconds into the stage.
    field: vec4f,
    // The notch indicator's done and failed, 0 to 1.
    ears: vec4f,
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

// `hud_placement::EAR_WIDTH`: how far each ear reaches past the notch, pt.
const EAR_WIDTH: f32 = 34.0;
const TAU: f32 = 6.2831853;

// Apple's dark-appearance system colours: the island is black in either
// appearance, as the notch is.
const SYSTEM_ORANGE: vec3f = vec3f(1.0, 0.624, 0.039);
const SYSTEM_GREEN: vec3f = vec3f(0.188, 0.820, 0.345);
const SYSTEM_RED: vec3f = vec3f(1.0, 0.271, 0.227);
const LABEL: vec3f = vec3f(0.92, 0.92, 0.94);

fn segment(point: vec2f, a: vec2f, b: vec2f, radius: f32) -> f32 {
    let pa = point - a;
    let ba = b - a;
    let t = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * t) - radius;
}

// SF Symbols' mic.fill, 13 pt tall: the capsule, its cradle, and stand.
fn mic(p: vec2f) -> f32 {
    let body = rounded_box(p - vec2f(0.0, -2.3), vec2f(2.35, 4.1), 2.35);
    // The cradle's middle is 1.2 pt above the glyph's.
    let ring = abs(length(p - vec2f(0.0, -1.2)) - 4.5) - 0.62;
    let cradle = min(max(ring, -1.2 - p.y), length(vec2f(abs(p.x) - 4.5, p.y + 1.2)) - 0.62);
    let stem = rounded_box(p - vec2f(0.0, 4.75), vec2f(0.62, 1.1), 0.62);
    let stand = rounded_box(p - vec2f(0.0, 6.0), vec2f(2.7, 0.62), 0.62);
    return min(min(body, cradle), min(stem, stand));
}

// SF Symbols' checkmark, bold.
fn checkmark(p: vec2f) -> f32 {
    let knee = vec2f(-1.2, 2.9);
    return min(segment(p, vec2f(-4.2, -0.2), knee, 0.95), segment(p, knee, vec2f(4.3, -3.6), 0.95));
}

// SF Symbols' exclamationmark: the stroke and its point.
fn exclamation(p: vec2f) -> f32 {
    return min(segment(p, vec2f(0.0, -3.3), vec2f(0.0, 0.8), 0.9), length(p - vec2f(0.0, 3.1)) - 1.0);
}

// Labs' notch indicator, in the Dynamic Island's compact style: the notch
// widened sideways at its own height, one glyph either side of the camera.
// Recording: an orange mic (the colour macOS and iOS use for a microphone
// in use) and capsule level bars that move with the voice. Working: the
// macOS 12-spoke spinner and the bars at rest with light running through
// them. Done: a green checkmark pops in. Failed: a red
// exclamationmark.circle.fill. Glyphs scale and unblur in as the ears open.
fn ears(window_point: vec2f, island_shape: f32) -> vec4f {
    let notch_half = u.field.x;
    let level = clamp(u.field.z, 0.0, 1.0);
    let clock = u.field.w;
    let done = clamp(u.ears.x, 0.0, 1.0);
    let fail = clamp(u.ears.y, 0.0, 1.0);
    let ear = max(u.island.x - notch_half, 0.0);
    let room = sstep(0.35, 1.0, ear / EAR_WIDTH);
    let s = mix(0.72, 1.0, room);
    let aa = (0.75 / u.backing_scale + (1.0 - room) * 1.4) / s;
    let working = clamp(u.cool, 0.0, 1.0);
    let done_in = done * sstep(0.0, 0.06, clock);
    let w_fail = fail;
    let w_done = done_in * (1.0 - fail);
    let w_work = working * (1.0 - done_in) * (1.0 - fail);
    let w_record = (1.0 - working) * (1.0 - fail);
    let middle_y = u.island.z + u.island.y * 0.5;
    let inset = notch_half + EAR_WIDTH * 0.5 - 1.0;
    var result = vec4f(0.0);

    // Leading: the mic, the spinner, the checkmark, or the failure.
    let lead = (window_point - vec2f(-inset, middle_y)) / s;
    composite(&result, SYSTEM_ORANGE, w_record * coverage(mic(lead), aa));
    if (w_work > 0.001) {
        let sector = TAU / 12.0;
        let k = round(atan2(lead.x, -lead.y) / sector);
        let direction = vec2f(sin(k * sector), -cos(k * sector));
        let along = clamp(dot(lead, direction), 2.7, 6.1);
        let spoke = length(lead - direction * along) - 0.75;
        let head = floor(clock * 12.0);
        let behind = (head - k) - 12.0 * floor((head - k) / 12.0);
        let shade = 0.2 + 0.8 * pow(1.0 - behind / 12.0, 1.5);
        composite(&result, LABEL, w_work * shade * coverage(spoke, aa));
    }
    if (w_done > 0.001) {
        let t = clamp(clock / 0.16, 0.0, 1.0);
        let pop = max(sstep(0.0, 1.0, t) * (1.0 + 0.2 * sin(t * 3.14159)), 0.05);
        composite(&result, SYSTEM_GREEN, w_done * coverage(checkmark(lead / pop), aa / pop));
    }
    if (w_fail > 0.001) {
        let disc = coverage(length(lead) - 6.4, aa) * (1.0 - coverage(exclamation(lead), aa));
        composite(&result, SYSTEM_RED, w_fail * disc);
    }

    // Trailing: the level bars, at rest with light running through them
    // while Hex works.
    let trail = (window_point - vec2f(inset, middle_y)) / s;
    let profile = array<f32, 5>(0.5, 0.8, 1.0, 0.75, 0.55);
    let rate = array<f32, 5>(1.7, 2.3, 1.9, 2.6, 2.1);
    let phase = array<f32, 5>(0.0, 1.3, 2.1, 0.7, 1.7);
    // Loudness reads as its square root, so ordinary speech moves the bars.
    let heard = sqrt(level);
    for (var i = 0; i < 5; i++) {
        let n = f32(i);
        let wobble = 0.6 + 0.4 * sin(TAU * clock * rate[i] * 0.5 + phase[i]);
        let height = 2.4 + 10.0 * heard * profile[i] * wobble * w_record;
        let bar = rounded_box(trail - vec2f((n - 2.0) * 4.0, 0.0), vec2f(1.1, height * 0.5), 1.1);
        let light = 0.3 + 0.7 * pow(0.5 + 0.5 * cos(TAU * (clock * 1.1 - n * 0.14)), 3.0);
        let color = mix(SYSTEM_ORANGE, LABEL, working);
        let alpha = w_record + w_work * light;
        composite(&result, color, alpha * coverage(bar, aa));
    }

    result *= u.opacity * room * island_shape;
    result.a += island_shape * (1.0 - result.a);
    return result;
}

// `var<immediate> u: DotUniforms;` on a device with immediates
// (`Painter::new`).
@group(0) @binding(0) var<uniform> u: DotUniforms;
// `MAX_DOTS` (1024) dots, two u16 halves to a u32, four u32 to a vec4.
@group(0) @binding(1) var<uniform> dots: array<vec4u, 256>;

fn dot_value(index: i32) -> vec2f {
    let word = dots[index / 4][index % 4];
    return vec2f(f32(word & 0xffffu), f32(word >> 16u)) / 65535.0;
}

@fragment
fn dot_scope_fragment(in: Varyings) -> @location(0) vec4f {
    let uv = in.uv;
    let position = uv * u.resolution;
    let scale = max(u.scale, 0.001) * max(u.plate_layout.z, 0.01);
    let window_point = (uv * u.resolution - u.resolution * 0.5) / u.backing_scale;
    let point = (window_point - u.plate_layout.xy) / scale;
    let pixel = 1.0 / (u.backing_scale * scale);
    let softness = max(u.softness, 0.0);
    let aa = pixel * 0.75 + softness * 0.72;
    let half_size = vec2f(u.width, u.height) * 0.5;
    let radius = clamp(u.field.x, 0.0, u.height * 0.5);
    let distance = rounded_box(point, half_size, radius);
    let dark = u.dark > 0.5;
    var result = vec4f(0.0);
    let in_island = u.plate_layout.w > 0.5;
    var island_shape = 1.0;
    if (in_island) {
        let island_aa = 0.75 / u.backing_scale;
        island_shape = coverage(island_distance(window_point, u.island), island_aa);
        if (u.field.y > 0.5) {
            return for_target(dithered(ears(window_point, island_shape), position));
        }
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
            let cap = -rounded_box(center, half_size, radius);
            let mask = sstep(1.4, 4.6, cap);
            if (mask <= 0.0) {
                continue;
            }
            let value = dot_value(j * columns + i);
            let power = max(value.x, 1e-5);
            let fresh = value.y;
            let row = abs(center.y) / half_size.y;
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
    return for_target(dithered(result, position));
}
