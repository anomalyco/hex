// `"With Hex"`'s fireworks (fireworks.rs): spark streaks with a hot core
// and a soft glow, and the bursts' light falling on the page.
//
// Colours arrive linear and brighter than white while hot. Dark pages take
// the sparks additively (alpha 0) and the light as `dst + dst · light`, so
// a surface brightens by its own colour and the near-black page barely
// does. Light pages take sparks as paint and the light as a tint.

const LIGHTS: u32 = 4u;

struct Slot {
    // The slot element's rect in window pixels (x, y, w, h).
    rect: vec4f,
    // This pass's origin in window pixels, then its size.
    view: vec4f,
    // Pixels per point, sRGB-encoded blending, dark page, light count.
    info: vec4f,
    // Per light: (x, y, reach, strength) in slot points, then (rgb, 0).
    lights: array<vec4f, 8>,
}

struct Item {
    // Tail (x, y), head (x, y), slot points.
    seg: vec4f,
    // Linear colour, then how bright the tail is next to the head.
    color: vec4f,
    // Core half width, glow radius (points), 0, 0.
    shape: vec4f,
    // Bounds in points: x0, y0, x1, y1.
    bounds: vec4f,
}

@group(0) @binding(0) var<uniform> slot: Slot;
@group(0) @binding(1) var<storage, read> items: array<Item>;

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

@vertex
fn spark_vertex(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Varyings {
    let b = items[i].bounds;
    return place(mix(b.xy, b.zw, corner(v)), i);
}

// The whole slot.
@vertex
fn area_vertex(@builtin(vertex_index) v: u32) -> Varyings {
    let k = max(slot.info.x, 1e-3);
    return place(corner(v) * slot.rect.zw / k, 0u);
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
    let peak = max(light.r, max(light.g, light.b));
    if peak <= 1e-5 {
        return vec4f(0.0);
    }
    let a = clamp(peak * 1.4, 0.0, 1.0);
    // Deeper than the light's own hue: pale gold is lost on a white page.
    let ink = pow(light / peak, vec3f(1.6)) * 0.85;
    return vec4f(ink * a, a);
}

@fragment
fn spark_fragment(v: Varyings) -> @location(0) vec4f {
    let item = items[v.item];
    let k = slot.info.x;
    let a = item.seg.xy;
    let b = item.seg.zw;
    let ab = b - a;
    let len2 = dot(ab, ab);
    let s = select(1.0, clamp(dot(v.at - a, ab) / max(len2, 1e-6), 0.0, 1.0), len2 > 1e-6);
    let d = length(v.at - (a + ab * s));
    let half = item.shape.x;
    let glow = max(item.shape.y, 0.5);
    // A one-pixel antialiased core, thinned to a dot's worth of light when
    // it's narrower than a pixel.
    let px = 0.6 / k;
    let core = (1.0 - smoothstep(half - px, half + px, d)) * min(half * k * 1.2, 1.0);
    // The glow goes to nothing before its quad's edge.
    let halo = exp(-d * d / (2.0 * glow * glow)) * (1.0 - smoothstep(glow * 1.8, glow * 3.0, d));
    // Brightest at the head, fading along the tail.
    let along = mix(item.color.w, 1.0, s * s);
    let light = item.color.rgb * (core + 0.32 * halo) * along;
    return out_color(emit(light));
}

// The bursts' light at a point, and how bright it is overall.
fn light_at(at: vec2f) -> vec3f {
    var sum = vec3f(0.0);
    let n = min(u32(slot.info.w + 0.5), LIGHTS);
    for (var i = 0u; i < n; i++) {
        let l = slot.lights[i * 2u];
        let c = slot.lights[i * 2u + 1u].rgb;
        let r = length(at - l.xy) / max(l.z, 1.0);
        // Close in, it's strong; far off, it fades smoothly to nothing,
        // with no edge to see.
        let near = 1.0 / (1.0 + 2.0 * r * r);
        let fall = near * near;
        sum += c * l.w * fall;
    }
    return sum;
}

// Dark page: `dst + dst · light`.
@fragment
fn lit_fragment(v: Varyings) -> @location(0) vec4f {
    return vec4f(light_at(v.at), 0.0);
}

// Light page: white surfaces take the light's hue, a little.
@fragment
fn tint_fragment(v: Varyings) -> @location(0) vec4f {
    let l = light_at(v.at);
    let peak = max(l.r, max(l.g, l.b));
    if peak <= 1e-4 {
        return vec4f(1.0);
    }
    let hue = l / peak;
    // A soft shoulder: a clamp here paints a flat disc of colour over the
    // page round each burst.
    let amount = 0.2 * (1.0 - exp(-0.45 * peak));
    return vec4f(mix(vec3f(1.0), hue, amount), 1.0);
}
