// The practice game's strike and burn (fx.rs). Ported from
// Psychopomp's dissolve.wgsl (burn field, scorched band, hot rim) and
// lightning.wgsl (white core, sheath, Lorentzian corona).
//
// Colours arrive linear. Dark pages take light additively (alpha 0); light
// pages take it as paint, since light on white doesn't show.

struct Slot {
    // The slot element's rect in window pixels (x, y, w, h).
    rect: vec4f,
    // This pass's origin in window pixels, then its size.
    view: vec4f,
    // Pixels per point, sRGB-encoded blending, dark page, bleed in points.
    info: vec4f,
    core: vec4f,
    corona: vec4f,
    scorch: vec4f,
    ash: vec4f,
    // Burn: impact (card points), card size (points).
    burn: vec4f,
    // Burn: age (s), seed, 0, 0.
    burn2: vec4f,
}

struct Item {
    // Bolt: first point, point count, core half width, reach (points).
    // Mote: x, y, radius, heat.
    a: vec4f,
    // Bolt: core, glow, drawn length (points), corona radius.
    // Mote: opacity.
    b: vec4f,
    // Bounds in points: x0, y0, x1, y1.
    c: vec4f,
}

@group(0) @binding(0) var<uniform> slot: Slot;
@group(0) @binding(1) var<storage, read> items: array<Item>;
@group(0) @binding(2) var<storage, read> points: array<vec2f>;

const EDGE: f32 = 0.04;
const BURN: f32 = 0.95;
const GRAIN: f32 = 0.035;
const SHAPE: f32 = 1.35;

struct Varyings {
    @builtin(position) position: vec4f,
    // Points in the slot element's own coordinates.
    @location(0) at: vec2f,
    @location(1) @interpolate(flat) item: u32,
}

fn corner(i: u32) -> vec2f {
    var c = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
    );
    return c[i];
}

fn place(at: vec2f, item: u32) -> Varyings {
    let k = slot.info.x;
    let window = slot.rect.xy + at * k;
    let local = (window - slot.view.xy) / slot.view.zw;
    var out: Varyings;
    out.position = vec4f(local.x * 2.0 - 1.0, 1.0 - local.y * 2.0, 0.0, 1.0);
    out.at = at;
    out.item = item;
    return out;
}

// The card and its bleed.
@vertex
fn burn_vertex(@builtin(vertex_index) v: u32) -> Varyings {
    let bleed = slot.info.w;
    let size = slot.burn.zw;
    let at = mix(vec2f(-bleed), size + vec2f(bleed), corner(v));
    return place(at, 0u);
}

// One bolt or mote, over its bounds.
@vertex
fn item_vertex(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Varyings {
    let c = items[i].c;
    return place(mix(c.xy, c.zw, corner(v)), i);
}

fn encode(c: vec3f) -> vec3f {
    let x = max(c, vec3f(0.0));
    return select(1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3f(0.0031308));
}

// Premultiplied colour as the pass blends it.
fn out_color(c: vec4f) -> vec4f {
    if slot.info.y < 0.5 {
        return c;
    }
    if c.a <= 0.0 {
        return vec4f(encode(c.rgb), 0.0);
    }
    return vec4f(encode(c.rgb / c.a) * c.a, c.a);
}

// Light: added on a dark page, painted on a light one.
fn emit(light: vec3f) -> vec4f {
    if slot.info.z > 0.5 {
        return vec4f(light, 0.0);
    }
    // The light's hue at a fixed depth, its strength as coverage.
    let peak = max(light.r, max(light.g, light.b));
    if peak <= 1e-5 {
        return vec4f(0.0);
    }
    let a = clamp(peak * 1.6, 0.0, 1.0);
    return vec4f(light / peak * 0.85 * a, a);
}

// Integer-hashed lattice noise, bit-identical to fx.rs.
fn lattice_hash(value: u32, salt: u32) -> f32 {
    var h = (value * 0x9E3779B1u) ^ (salt * 0x85EBCA77u);
    h = h ^ (h >> 15u);
    h = h * 0x2C1B3C6Du;
    h = h ^ (h >> 12u);
    h = h * 0x297A2D39u;
    h = h ^ (h >> 15u);
    return f32(h >> 8u) / 16777216.0;
}

fn lattice_at(cell: vec2<i32>, salt: u32) -> f32 {
    return lattice_hash(bitcast<u32>(cell.x) + bitcast<u32>(cell.y) * 0x27D4EB2Fu, salt);
}

fn lattice2(p: vec2f, salt: u32) -> f32 {
    let c = floor(p);
    let f = p - c;
    let i = vec2<i32>(c);
    let u = f * f * (3.0 - 2.0 * f);
    let bottom = mix(lattice_at(i, salt), lattice_at(i + vec2<i32>(1, 0), salt), u.x);
    let top = mix(lattice_at(i + vec2<i32>(0, 1), salt), lattice_at(i + vec2<i32>(1, 1), salt), u.x);
    return mix(bottom, top, u.y);
}

fn fbm(p: vec2f, salt: u32) -> f32 {
    var q = p;
    var sum = 0.0;
    var amplitude = 0.5;
    for (var octave = 0u; octave < 4u; octave++) {
        sum += lattice2(q, salt + octave) * amplitude;
        q = q * 2.03 + vec2f(17.0, 5.0);
        amplitude *= 0.5;
    }
    return sum / 0.9375;
}

// The field the front sweeps, 0..1: out from the impact, through noise.
fn field(local: vec2f, size: vec2f, impact: vec2f, seed: u32) -> f32 {
    let far = max(
        max(length(impact), length(impact - vec2f(size.x, 0.0))),
        max(length(impact - vec2f(0.0, size.y)), length(impact - size)),
    );
    let radial = length(local - impact) / max(far, 1.0);
    let noise = clamp((fbm(local * GRAIN, seed) - 0.2) / 0.6, 0.0, 1.0);
    return clamp(0.75 * radial + 0.25 * noise, 0.0, 1.0);
}

fn front(age: f32) -> f32 {
    let u = clamp(age / BURN, 0.0, 1.0);
    return (1.0 - pow(1.0 - u, SHAPE)) * (1.0 + 2.0 * EDGE) - EDGE;
}

// (remaining, rim, scorch, lead) at a point. The field runs on past the
// card's edge, so its shadow in the bleed burns away with it.
fn burn_at(at: vec2f) -> vec4f {
    let f = field(at, slot.burn.zw, slot.burn.xy, u32(slot.burn2.y));
    let aa = max(fwidth(f), 1e-4);
    let lead = f - front(slot.burn2.x);
    let keep = smoothstep(-aa, aa, lead);
    let rim = keep * (1.0 - smoothstep(0.0, EDGE, lead));
    let scorch = keep * (1.0 - smoothstep(EDGE, EDGE * 3.0, lead));
    return vec4f(keep, rim, scorch, lead);
}

// The rim's light: corona at its cool side, white-hot at the front.
fn rim_light(rim: f32) -> vec3f {
    let hot = rim * rim;
    return mix(slot.corona.rgb * 0.8, slot.core.rgb * 1.2, hot * hot) * rim * 1.2;
}

// Drawn over the card's own layer with source-atop blending: the scorched
// band darkens the card ahead of the front and the rim glows on it.
@fragment
fn scorch_fragment(v: Varyings) -> @location(0) vec4f {
    let b = burn_at(v.at);
    // Only on the card, not its shadow in the bleed.
    let size = slot.burn.zw;
    let outside = length(v.at - clamp(v.at, vec2f(0.0), size));
    let on = 1.0 - smoothstep(0.0, 1.0, outside);
    let flicker = 0.75 + 0.5 * lattice2(v.at * 0.18 + vec2f(slot.burn2.x * 9.0, 0.0), 77u);
    let soot = 0.8 * max(b.z - b.y, 0.0) * on;
    let light = emit(rim_light(b.y) * flicker * on);
    return out_color(vec4f(slot.scorch.rgb * soot + light.rgb, max(soot, light.a)));
}

// Drawn with destination-out blending: what the front has passed is gone,
// so the page behind the card shows through.
@fragment
fn erase_fragment(v: Varyings) -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, 1.0 - burn_at(v.at).x);
}

// Drawn over: the burning edge lights the page around it, inside the hole
// and past the card's edge.
@fragment
fn halo_fragment(v: Varyings) -> @location(0) vec4f {
    let b = burn_at(v.at);
    let size = slot.burn.zw;
    let outside = length(v.at - clamp(v.at, vec2f(0.0), size));
    let near = exp(-b.w * b.w / (EDGE * EDGE * 4.0)) * exp(-outside / 2.5);
    // Light only while there's an edge to burn.
    let live = 1.0 - smoothstep(BURN * 0.85, BURN * 1.1, slot.burn2.x);
    let strength = select(0.15, 0.3, slot.info.z > 0.5);
    return out_color(emit(slot.corona.rgb * near * live * strength));
}

// The plasma optics of one channel at `d` px from its centre line.
fn channel(d: f32, half: f32, core: vec3f, corona: vec3f, radius: f32, reach: f32) -> vec3f {
    let edge = d - half;
    let body = clamp(0.5 - edge / 0.9, 0.0, 1.0);
    let outside = max(edge, 0.0);
    let sheath_width = half * 1.4 + 0.6;
    let sheath = exp(-outside * outside / (2.0 * sheath_width * sheath_width));
    let r = d / max(radius, 0.5);
    let scatter = (1.0 / (1.0 + r * r)) * (1.0 - smoothstep(reach * 0.5, reach, d));
    return core * body + mix(core, corona, 0.75) * (sheath * 0.2) + corona * (scatter * 0.45);
}

@fragment
fn bolt_fragment(v: Varyings) -> @location(0) vec4f {
    let item = items[v.item];
    let first = u32(item.a.x + 0.5);
    let count = u32(item.a.y + 0.5);
    let drawn = item.b.z;
    var best = 1e9;
    var along = 0.0;
    var walked = 0.0;
    for (var i = 0u; i + 1u < count; i++) {
        let a = points[first + i];
        let b = points[first + i + 1u];
        let ab = b - a;
        let len = length(ab);
        if walked >= drawn {
            break;
        }
        let shown = min(len, drawn - walked);
        let dir = ab / max(len, 1e-4);
        let s = clamp(dot(v.at - a, dir), 0.0, shown);
        let d = length(v.at - (a + dir * s));
        if d < best {
            best = d;
            along = walked + s;
        }
        walked += len;
    }
    let k = slot.info.x;
    let total = max(walked, 1.0);
    let taper = 1.0 - 0.45 * clamp(along / total, 0.0, 1.0);
    let core = slot.core.rgb * item.b.x + slot.corona.rgb * item.b.y;
    let corona = slot.corona.rgb * (item.b.x + item.b.y);
    let light = channel(best * k, item.a.z * k * taper, core, corona, item.b.w * k, item.a.w * k);
    return out_color(emit(light));
}

// An ash flake: glowing as it leaves the rim, cooling to a grey fleck.
@fragment
fn mote_fragment(v: Varyings) -> @location(0) vec4f {
    let item = items[v.item];
    let k = slot.info.x;
    let r = max(item.a.z, 0.3);
    let d = length(v.at - item.a.xy);
    let cover = 1.0 - smoothstep(r - 0.6 / k, r + 0.6 / k, d);
    let heat = item.a.w;
    let opacity = item.b.x;
    let glow = exp(-d * d / (r * r * 1.5));
    let hot = mix(slot.corona.rgb, slot.core.rgb, heat * heat);
    let light = emit(hot * heat * (cover * 1.2 + glow * 0.5) * opacity);
    let paint = slot.ash.a * cover * (1.0 - heat) * opacity;
    return out_color(vec4f(slot.ash.rgb * paint + light.rgb, max(paint, light.a)));
}
