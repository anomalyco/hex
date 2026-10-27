// The welcome hero: the Hex glyph, backlit. Two passes over a quad around the
// stage, drawn after the UI:
//
// 1. `fs_rays`, at reduced resolution into its own texture: the lamp behind the
//    glyph seen through the air, integrated along each pixel's path to the lamp
//    and masked by the icon's exact distance field (god rays, as rune-light's
//    `shadow.wgsl` `shaft`: Mitchell, GPU Gems 3 ch. 13, done in the field, the
//    mask prefiltered to the sample spacing). Lit parts of the glyph glow in
//    the same air.
// 2. `fs_composite`, at full resolution over the target: the glyph with
//    bevel normals, a rim from the lamp, a key light's specular, the lit
//    parts' emission and their light on what is next to them, bloom, and the
//    rays. rune-light's shoulder tone curve and interleaved-gradient dither.
//
// Everything is a pure function of (pixel, uniforms); the springs that drive
// the uniforms are sampled on the CPU (`hero.rs`). Positions are in points
// from the icon's centre, y down. Colours are linear.

struct U {
    // The drawn quad in target pixels: x, y, w, h.
    quad: vec4f,
    // Target width and height in pixels, device scale, seconds since the entrance began.
    view: vec4f,
    // Opacity Rune presents the stage at, dark (0/1), still (0/1), encode to sRGB (0/1).
    state: vec4f,
    // The glyph's centre in target pixels, its outer apothem and the stage's half height in points.
    centre: vec4f,
    page: vec4f,
    ink: vec4f,
    // The lamp: linear colour, intensity (rise, swells and breath applied).
    lamp: vec4f,
    // The lamp's position (points from the centre), its glow radius, and its disc radius.
    lamp_at: vec4f,
    // Emission: linear colour, unused.
    emit: vec4f,
    // Each part's emission (outer ring, middle ring, core, as `glyph3`), unused.
    ignite: vec4f,
    // Streaks: strength, sharpness, unused, unused.
    streak: vec4f,
    // Light mode: tint, strength.
    tint: vec4f,
    // Rays: halo weight, disc weight, emission weight, reach in points.
    rays: vec4f,
}

@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var rays_tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct V {
    @builtin(position) pos: vec4f,
    @location(0) p: vec2f,
    @location(1) uv: vec2f,
}

fn t() -> f32 { return u.view.w; }
fn scale() -> f32 { return u.view.z; }
fn dark() -> bool { return u.state.y > 0.5; }

// The quad's corner `c` (0..1) in points from the icon's centre.
fn at(c: vec2f) -> vec2f {
    return (u.quad.xy + c * u.quad.zw - u.centre.xy) / scale();
}

fn corner(i: u32) -> vec2f {
    return vec2f(f32(i & 1u), f32((i >> 1u) & 1u));
}

// The rays pass draws the whole of its own texture.
@vertex
fn vs_rays(@builtin(vertex_index) i: u32) -> V {
    let c = corner(i);
    var o: V;
    o.pos = vec4f(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    o.p = at(c);
    o.uv = c;
    return o;
}

// The composite draws the quad over the target.
@vertex
fn vs_quad(@builtin(vertex_index) i: u32) -> V {
    let c = corner(i);
    let px = u.quad.xy + c * u.quad.zw;
    let n = px / u.view.xy * 2.0 - 1.0;
    var o: V;
    o.pos = vec4f(n.x, -n.y, 0.0, 1.0);
    o.p = at(c);
    o.uv = c;
    return o;
}

// ---------------------------------------------------------------- helpers

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

fn sq(x: f32) -> f32 { return x * x; }

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn hash(n: u32) -> f32 { return f32(pcg(n) >> 8u) * (1.0 / 16777216.0); }

// Value noise around a circle of `n` cells, so it wraps without a seam.
fn ring_noise(x: f32, n: u32, seed: u32) -> f32 {
    let i = floor(x);
    let f = x - i;
    let a = u32(i32(i) + 1000000) % n;
    let b = (a + 1u) % n;
    return mix(hash(a * 7u + seed), hash(b * 7u + seed), f * f * (3.0 - 2.0 * f));
}

// Interleaved gradient noise (Jimenez): a fixed per-pixel pattern, as rune-light dithers.
fn ign(p: vec2f) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2f(0.06711056, 0.00583715))));
}

// rune-light's tone curve: linear to 0.8, then a soft shoulder to 1.
fn shoulder(x: vec3f) -> vec3f {
    let k = 0.8;
    let over = max(x - vec3f(k), vec3f(0.0));
    return min(x, vec3f(k)) + (1.0 - k) * (vec3f(1.0) - exp(-over / (1.0 - k)));
}

fn srgb(c: vec3f) -> vec3f {
    let s = clamp(c, vec3f(0.0), vec3f(1.0));
    return select(1.055 * pow(s, vec3f(1.0 / 2.4)) - 0.055, s * 12.92, s <= vec3f(0.0031308));
}

fn linear(c: vec3f) -> vec3f {
    let s = clamp(c, vec3f(0.0), vec3f(1.0));
    return select(pow((s + 0.055) / 1.055, vec3f(2.4)), s / 12.92, s <= vec3f(0.04045));
}

fn encode(c: vec3f) -> vec3f {
    if u.state.w < 0.5 {
        return c;
    }
    return srgb(c);
}

// A linear colour in the target's encoding, dithered by `d` in sRGB steps:
// an sRGB target stores linear values, where a step near black is many.
fn dithered(c: vec3f, d: f32) -> vec3f {
    let s = srgb(c) + vec3f(d);
    if u.state.w < 0.5 {
        return linear(s);
    }
    return s;
}

fn luma(c: vec3f) -> f32 { return dot(c, vec3f(0.2126, 0.7152, 0.0722)); }

fn ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// ---------------------------------------------------------------- the icon

// Pointy-top hexagon with apothem `a` (its vertical sides at x = ±a).
fn hex(p: vec2f, a: f32) -> f32 {
    let k = vec3f(-0.8660254, 0.5, 0.57735027);
    var q = abs(p.yx);
    q -= 2.0 * min(dot(k.xy, q), 0.0) * k.xy;
    q -= vec2f(clamp(q.x, -k.z * a, k.z * a), a);
    return length(q) * sign(q.y);
}

fn rhex(p: vec2f, a: f32, r: f32) -> f32 { return hex(p, a - r) - r; }

// Half a ring's width, in units of the glyph's outer apothem.
const RING: f32 = 0.0612;
// The parts' apothems: outer ring, middle ring, inner hexagon (measured from
// app/AppIcon.icon/Assets/Image.png).
const OUTER: f32 = 0.939;
const MIDDLE: f32 = 0.600;
const INNER: f32 = 0.297;

// Signed distances to the outer ring, middle ring and inner hexagon, in glyph units.
fn glyph3(p: vec2f) -> vec3f {
    return vec3f(
        abs(rhex(p, OUTER, 0.138)) - RING,
        abs(rhex(p, MIDDLE, 0.088)) - RING,
        rhex(p, INNER, 0.071),
    );
}

fn glyph(p: vec2f) -> f32 {
    let g = glyph3(p);
    return min(min(g.x, g.y), g.z);
}

fn glyph_grad(p: vec2f) -> vec2f {
    let e = vec2f(0.004, 0.0);
    let g = vec2f(glyph(p + e.xy) - glyph(p - e.xy), glyph(p + e.yx) - glyph(p - e.yx));
    return g / max(length(g), 1e-5);
}

// The glyph's outer apothem in points.
fn glyph_size() -> f32 { return u.centre.z; }

// The glyph's parts with sharp corners, for the rays' march: the hexagonal
// norm is a distance along each edge's normal, a little short near corners.
fn glyph3_fast(p: vec2f) -> vec3f {
    let h = hexn(p);
    return vec3f(abs(h - OUTER) - RING, abs(h - MIDDLE) - RING, h - INNER);
}

// What stands between the lamp and the viewer, as a distance in points.
fn occluder(p: vec2f) -> f32 {
    let g = glyph_size();
    let d = glyph3_fast(p / g);
    return min(min(d.x, d.y), d.z) * g;
}

// The lamp seen directly: a soft glow (windowed like rune-light's point light)
// and a disc just larger than the icon, which peeks around its edge.
fn lamp_glow(r: f32) -> f32 {
    let R = u.lamp_at.z;
    let halo = R * R / (r * r + R * R);
    let disc = 1.0 - smoothstep(u.lamp_at.w - 10.0, u.lamp_at.w + 14.0, r);
    return u.rays.x * halo + u.rays.y * disc;
}

// Haze in the air the rays cross: still streaks around the lamp.
fn haze(q: vec2f) -> f32 {
    let d = q - u.lamp_at.xy;
    let a = atan2(d.y, d.x) / TAU + 0.5;
    let n = 0.5 * ring_noise(a * 37.0, 37u, 11u) + 0.3 * ring_noise(a * 97.0, 97u, 23u) + 0.2 * ring_noise(a * 233.0, 233u, 31u);
    let s = pow(clamp(n * 1.25, 0.0, 1.0), u.streak.y);
    return mix(1.0, s * 1.8, u.streak.x);
}

// The lit parts' radiance at `q`, prefiltered to `w` points.
fn emission(q: vec2f, w: f32) -> f32 {
    let g = glyph_size();
    let d = glyph3_fast(q / g) * g;
    let cover = 1.0 - smoothstep(-vec3f(w), vec3f(w), d);
    return dot(cover, u.ignite.xyz);
}

// ================================================================ pass 1: the rays

const SAMPLES: i32 = 32;

@fragment
fn fs_rays(i: V) -> @location(0) vec4f {
    let p = i.p;
    let L = u.lamp_at.xy;
    let D = L - p;
    let len = length(D);
    let stride = len / f32(SAMPLES);
    let w = max(stride * 0.6, 0.75);
    let j = ign(i.pos.xy);
    // Every sample lies on the same line through the lamp, so the haze's
    // streaks are the same all along it.
    let streaks = haze(p);
    let glowing = u.rays.z > 0.0;
    var air = 0.0;
    var glow = 0.0;
    for (var k = 0; k < SAMPLES; k++) {
        let q = p + D * ((f32(k) + j) / f32(SAMPLES));
        let open = smoothstep(-w, w, occluder(q));
        air += open * lamp_glow(length(q - L));
        if glowing {
            glow += emission(q, w);
        }
    }
    air *= streaks / f32(SAMPLES);
    glow /= f32(SAMPLES);
    // Distance falloff, as rune-light's shafts: the air thins away from the lamp.
    let fall = sq(u.rays.w / (len + u.rays.w));
    let rgb = u.lamp.rgb * u.lamp.w * air * fall + u.emit.rgb * u.rays.z * glow * streaks * fall;
    return vec4f(rgb, 1.0);
}

// ================================================================ pass 2: the icon and the light

// Where the hero ends: an ellipse around the icon, and above the title.
fn room(p: vec2f) -> f32 {
    let e = 1.0 - smoothstep(0.5, 1.0, length(p / vec2f(380.0, 236.0)));
    let below = sq(1.0 - smoothstep(0.0, u.centre.w + 20.0, p.y));
    return e * below;
}

// Where light comes from in front, in the plane (y down): above and a little left.
const KEY: vec3f = vec3f(-0.30, -0.62, 0.72);

// One part's surface: its bevel normal, how far into its tube, and its emission.
struct Part {
    n: vec3f,
    // 0 at the edge, 1 along the middle of a ring or across the core's face.
    depth: f32,
    cover: f32,
    e: f32,
}

fn part(p: vec2f) -> Part {
    let g = glyph_size();
    let px = 1.0 / scale();
    let q = p / g;
    let d3 = glyph3(q) * g;
    let cover3 = 1.0 - smoothstep(-vec3f(px), vec3f(px), d3);
    var o: Part;
    o.cover = max(max(cover3.x, cover3.y), cover3.z);
    if o.cover <= 0.0 {
        return o;
    }
    let ring = min(d3.x, d3.y) < d3.z;
    let dg = min(min(d3.x, d3.y), d3.z);
    let n2 = glyph_grad(q);
    o.depth = clamp(-dg / select(g * 0.07, g * RING, ring), 0.0, 1.0);
    // Rings are round tubes; the core is a slab with a bevelled edge.
    let slope = select(1.6 * sq(1.0 - o.depth), 2.2 * (1.0 - o.depth), ring);
    o.n = normalize(vec3f(n2 * slope, 1.0));
    o.e = dot(cover3, u.ignite.xyz) / max(cover3.x + cover3.y + cover3.z, 1e-3);
    return o;
}

// The glyph's colour where it covers `p`, lit.
fn glyph_color(p: vec2f, pt: Part, unlit: vec3f) -> vec3f {
    let g = glyph_size();
    let h = normalize(KEY + vec3f(0.0, 0.0, 1.0));
    let nl = max(dot(pt.n, KEY), 0.0);
    let spec = ggx(max(dot(pt.n, h), 0.0), 0.32) * 0.05;
    // The glyph is a silhouette against the lamp: its glow wraps round the
    // bevels that face away from it.
    let d = p - u.lamp_at.xy;
    let R = u.lamp_at.z;
    let halo = R * R / (dot(d, d) + R * R);
    let rim = pow(1.0 - pt.n.z, 2.0) * max(dot(pt.n.xy, normalize(d + vec2f(1e-3))), 0.0) * halo * u.lamp.w;
    var c = unlit * (0.35 + 0.65 * nl) + vec3f(spec) + u.lamp.rgb * rim * 0.9;
    let lit = clamp(pt.e, 0.0, 1.0);
    if !dark() {
        // On a light page a lit part turns to ink, its bevel still catching the key light.
        let ink = u.ink.rgb * (0.55 + 0.45 * nl) + vec3f(spec);
        return mix(c, ink, lit);
    }
    // A lit part: a bright tube, hottest along its middle, keeping its specular.
    let hot = u.emit.rgb * pt.e * (0.62 + 0.5 * pt.depth);
    c = c * (1.0 - 0.7 * lit) + hot + vec3f(spec) * lit;
    // Light from the part inside reaches the bevel facing it.
    let inward = max(dot(pt.n.xy, -normalize(p + vec2f(1e-3))), 0.0) * (1.0 - pt.n.z);
    let m = hexn(p / g);
    let inner = select(u.ignite.z, u.ignite.y, m > (MIDDLE + OUTER) * 0.5);
    c += u.emit.rgb * inward * inner * 1.6;
    return c;
}

// The apothem of the pointy-top hexagon through `p`.
fn hexn(p: vec2f) -> f32 {
    let q = abs(p);
    return max(q.x, 0.5 * q.x + 0.8660254 * q.y);
}

// The glyph's light around each lit part, as bloom in the air.
fn spill(p: vec2f, near: f32, far: f32) -> f32 {
    let g = glyph_size();
    let d = max(glyph3(p / g) * g, vec3f(0.0));
    return dot(u.ignite.xyz, near * exp(-d / 2.6) + far * exp(-d / 11.0) + far * 0.35 * exp(-d / 34.0));
}

@fragment
fn fs_composite(i: V) -> @location(0) vec4f {
    let p = i.p;
    let rays = textureSample(rays_tex, samp, i.uv).rgb;
    let mask = room(p);
    let dn = (ign(i.pos.xy) + ign(i.pos.yx + vec2f(17.0, 3.0)) - 1.0) / 255.0;

    // The glyph, lit.
    let pt = part(p);
    let cover = pt.cover;
    var surface = vec3f(0.0);
    if cover > 0.0 {
        let unlit = select(mix(u.page.rgb, u.ink.rgb, 0.16), vec3f(0.012), dark());
        surface = glyph_color(p, pt, unlit);
    }

    // The air: the rays, and bloom around the lit parts.
    var air = rays;
    air += u.emit.rgb * spill(p, 0.10, 0.07);
    air *= mask;

    var o = vec4f(0.0);
    if dark() {
        // Light adds to the page: its change, in the target's encoding, so
        // blending stays exact over the page and graceful over anything else.
        let page = u.page.rgb;
        // Dither only where there is light, so the quad's edge never shows.
        let lit = dithered(shoulder(page + air), dn * min(luma(air) * 2000.0, 1.0)) - encode(page);
        let solid = dithered(shoulder(surface + air * 0.04), dn);
        o = vec4f(solid * cover + max(lit, vec3f(0.0)) * (1.0 - cover), cover);
    } else {
        // On a light page the light is a tint: it shows as colour, not brightness.
        let k = clamp(luma(air) * u.tint.w, 0.0, 0.62);
        let tinted = vec4f(encode(u.tint.rgb) * k, k);
        let solid = dithered(shoulder(surface + air * 0.2), dn);
        o = vec4f(solid * cover, cover) + tinted * (1.0 - cover);
    }
    return o * u.state.x;
}
