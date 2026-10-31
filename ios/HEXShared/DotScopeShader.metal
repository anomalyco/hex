#include <metal_stdlib>
using namespace metal;

// Port of `src/rune_hud.wgsl` + OpenCode iOS bar-windowing (`DotScopeShaders.metal`).
// Positions are in points from the view's center (x right, y down);
// colors are premultiplied display-sRGB written into a `.bgra8Unorm` layer.

struct DotScopeVaryings {
    float4 position [[position]];
    float2 uv;
};

struct DotScopeUniforms {
    float4 metrics0; // x: res.x, y: res.y, z: backingScale, w: pitch
    float4 metrics1; // x: width, y: height, z: opacity, w: scale
    float4 metrics2; // x: softness, y: columns, z: rows, w: radius
    float4 state;    // x: glow, y: grid, z: live, w: cool
    float4 window;   // x: leadingInset, y: trailingInset, z: windowFade, w: activeCenter
    float4 base;     // rgb: base color, w: drawPlate (1 = pill+rim+shadow, 0 = embedded)
    float4 core;     // rgb: core color, w: activeHalfWidth
    float4 plate;    // rgba
    float4 rim;      // rgba
    float4 shadow;   // rgba
};

static_assert(sizeof(DotScopeUniforms) == 160, "DotScopeUniforms must match Swift layout");

struct DotScopePackedDot {
    ushort power;
    ushort fresh;
};

struct DotSwipeCueUniforms {
    float4 metrics; // x: res.x, y: res.y, z: backingScale, w: pitch
    float4 grid;    // x: columns, y: rows, z: time, w: still
    float4 base;    // rgb: warm pearl base, w: opacity
    float4 core;    // rgb: warm pearl core, w: edgeFadePts
};

static_assert(sizeof(DotSwipeCueUniforms) == 64, "DotSwipeCueUniforms must match Swift layout");

static inline float ds_sstep(float edge0, float edge1, float x) {
    float t = clamp((x - edge0) / (edge1 - edge0), 0.0f, 1.0f);
    return t * t * (3.0f - 2.0f * t);
}

static inline float ds_rounded_box(float2 point, float2 half_size, float radius) {
    float2 q = abs(point) - half_size + radius;
    return length(max(q, float2(0.0f))) + min(max(q.x, q.y), 0.0f) - radius;
}

static inline float ds_coverage(float distance, float softness) {
    return ds_sstep(softness, -softness, distance);
}

static inline float ds_scope_window(float2 point, float2 half_size, float4 window) {
    float fade = window.z;
    if (fade <= 0.01f) {
        return 1.0f;
    }
    float width = half_size.x * 2.0f;
    float x_from_left = point.x + half_size.x;
    float x_from_right = width - x_from_left;
    float lead = window.x;
    float trail = window.y;

    float raw_l = ds_sstep(lead - 2.0f, lead + fade, x_from_left);
    float raw_r = ds_sstep(trail - 2.0f, trail + fade, x_from_right);
    float win_l = mix(1.0f, raw_l, clamp(lead / 8.0f, 0.0f, 1.0f));
    float win_r = mix(1.0f, raw_r, clamp(trail / 8.0f, 0.0f, 1.0f));
    return min(win_l, win_r);
}

static inline void ds_composite(thread float4 &destination, float3 color, float alpha_in) {
    float alpha = clamp(alpha_in, 0.0f, 1.0f);
    destination = float4(
        color * alpha + destination.rgb * (1.0f - alpha),
        alpha + destination.a * (1.0f - alpha)
    );
}

static inline float4 ds_dithered(float4 result_in, float2 position) {
    float4 result = result_in;
    if (result.a > 0.0f) {
        float2 pixel = floor(position);
        float n1 = fract(52.9829189f * fract(dot(pixel, float2(0.06711056f, 0.00583715f))));
        float n2 = fract(
            52.9829189f * fract(dot(pixel + float2(17.0f, 31.0f), float2(0.06711056f, 0.00583715f)))
        );
        float dither = ((n1 - n2) / 255.0f) * result.a;
        result = float4(clamp(result.rgb + float3(dither), float3(0.0f), float3(result.a)), result.a);
    }
    return result;
}

vertex DotScopeVaryings dot_scope_vertex(uint vertex_id [[vertex_id]]) {
    constexpr float2 positions[3] = {
        float2(-1.0f, -1.0f),
        float2( 3.0f, -1.0f),
        float2(-1.0f,  3.0f)
    };
    float2 p = positions[vertex_id];
    DotScopeVaryings out;
    out.position = float4(p, 0.0f, 1.0f);
    out.uv = float2(p.x * 0.5f + 0.5f, 0.5f - p.y * 0.5f);
    return out;
}

fragment float4 dot_scope_fragment(
    DotScopeVaryings in [[stage_in]],
    constant DotScopeUniforms &u [[buffer(0)]],
    constant DotScopePackedDot *dots [[buffer(1)]]
) {
    float2 resolution = u.metrics0.xy;
    float backing_scale = max(u.metrics0.z, 1.0f);
    float pitch = u.metrics0.w;
    float width = u.metrics1.x;
    float height = u.metrics1.y;
    float opacity = u.metrics1.z;
    float scale = max(u.metrics1.w, 0.001f);
    float softness = max(u.metrics2.x, 0.0f);
    int columns = int(u.metrics2.y + 0.5f);
    int rows = int(u.metrics2.z + 0.5f);
    float radius = clamp(u.metrics2.w, 0.0f, height * 0.5f);

    float glow = u.state.x;
    float grid = u.state.y;
    float live = u.state.z;
    float cool = u.state.w;
    bool draw_plate = u.base.w > 0.5f;
    float active_center = u.window.w;
    float active_half_width = max(u.core.w, 24.0f);

    float2 position = in.uv * resolution;
    float2 window_point = (position - resolution * 0.5f) / backing_scale;
    float2 point = window_point / scale;
    float pixel = 1.0f / (backing_scale * scale);
    float aa = pixel * 0.75f + softness * 0.72f;
    float2 half_size = float2(width, height) * 0.5f;
    float distance = ds_rounded_box(point, half_size, radius);

    float4 result = float4(0.0f);

    if (draw_plate) {
        float shadow_distance = ds_rounded_box(point - float2(0.0f, 3.0f), half_size, radius);
        ds_composite(result, u.shadow.rgb, u.shadow.a * (0.5f - 0.5f * tanh(shadow_distance / 6.5f)));
    }

    float shape = ds_coverage(distance, aa);

    if (draw_plate) {
        ds_composite(result, u.plate.rgb, u.plate.a * shape);
        ds_composite(result, u.rim.rgb, u.rim.a * ds_coverage(abs(distance) - 0.3f, aa));
    }

    float point_win = ds_scope_window(point, half_size, u.window);

    if (glow > 0.01f) {
        float reach_x = max(36.0f, active_half_width * 0.76f) * (0.86f + 0.28f * min(1.0f, glow));
        float reach_y = max(13.0f, half_size.y * 0.78f);
        float dx = (point.x - active_center) / reach_x;
        float dy = point.y / reach_y;
        float falloff = exp(-0.5f * (dx * dx + dy * dy * 1.55f));
        float strength = 0.20f * min(1.1f, glow);
        float3 middle = mix(u.base.rgb, u.core.rgb, 0.5f);
        ds_composite(result, mix(u.base.rgb, middle, falloff), strength * falloff * shape * point_win);
    }

    float2 center_offset = float2(float(columns) - 1.0f, float(rows) - 1.0f) * 0.5f;
    int i0 = int(rint(point.x / pitch + center_offset.x));
    int j0 = int(rint(point.y / pitch + center_offset.y));
    constexpr float unlit = 0.072f;
    constexpr float full = 0.96f;
    constexpr float curve = 0.90f;
    float presence = 0.5f + 0.5f * max(live, cool);

    for (int dj = -1; dj <= 1; dj++) {
        for (int di = -1; di <= 1; di++) {
            int i = i0 + di;
            int j = j0 + dj;
            if (i < 0 || j < 0 || i >= columns || j >= rows) {
                continue;
            }
            float2 center = (float2(float(i), float(j)) - center_offset) * pitch;
            float cap = -ds_rounded_box(center, half_size, radius);
            float mask = ds_sstep(1.6f, 5.2f, cap);
            if (mask <= 0.0f) {
                continue;
            }
            float win = ds_scope_window(center, half_size, u.window);
            if (win <= 0.001f) {
                continue;
            }
            float blur = 1.0f - win;
            DotScopePackedDot packed = dots[j * columns + i];
            float power = max(float(packed.power) / 65535.0f, 1e-5f);
            float fresh = float(packed.fresh) / 65535.0f;
            float row = abs(center.y) / max(half_size.y, 1.0f);
            float floor_alpha = unlit * (1.0f - 0.7f * ds_sstep(0.35f, 0.95f, row)) * presence * grid;
            float alpha = floor_alpha + (full - floor_alpha) * pow(power, curve * mix(1.4f, 1.0f, fresh));
            float dot_radius = (0.25f + 0.17f * pow(power, 0.6f))
                * mix(1.0f, mix(0.82f, 1.0f, fresh), ds_sstep(0.03f, 0.2f, power))
                * pitch
                * (1.0f + 0.28f * blur);
            float dot_aa = aa + 1.35f * blur;
            float core_mix = clamp(power * power * 1.1f, 0.0f, 0.9f) * mix(0.3f, 1.0f, fresh);
            float3 color = mix(u.base.rgb, u.core.rgb, core_mix);
            float from_center = length(point - center);
            float gate = mask * win * shape;
            if (power > 0.05f) {
                ds_composite(
                    result,
                    color,
                    0.132f * power * power * gate * ds_coverage(from_center - dot_radius * 2.0f, dot_aa)
                );
            }
            ds_composite(result, color, alpha * gate * ds_coverage(from_center - dot_radius, dot_aa));
        }
    }

    result *= opacity;
    return ds_dithered(result, position);
}

// ───────────────────── Directional Swipe Return Cue ─────────────────────
// A 5-row dot-matrix strip in the same LED style where a warm-pearl chevron
// comet sweeps left-to-right (`═══❯`) to show the bottom-edge return gesture.

fragment float4 dot_swipe_cue_fragment(
    DotScopeVaryings in [[stage_in]],
    constant DotSwipeCueUniforms &u [[buffer(0)]]
) {
    float2 resolution = u.metrics.xy;
    float backing_scale = max(u.metrics.z, 1.0f);
    float pitch = max(u.metrics.w, 1.5f);
    int columns = int(u.grid.x + 0.5f);
    int rows = int(u.grid.y + 0.5f);
    float time = u.grid.z;
    bool still = u.grid.w > 0.5f;
    float opacity = u.base.w;
    float edge_fade = max(u.core.w, 8.0f);

    float2 position = in.uv * resolution;
    float2 point = (position - resolution * 0.5f) / backing_scale;
    float width = resolution.x / backing_scale;
    float half_width = width * 0.5f;
    float aa = 0.75f / backing_scale;

    constexpr float period = 1.55f;
    constexpr float active_duty = 0.88f;
    float cycle = fract(time / period);
    float pass_u = still ? 0.52f : clamp(cycle / active_duty, 0.0f, 1.0f);
    float active_env = still ? 0.88f : (
        ds_sstep(0.0f, 0.09f, pass_u) * (1.0f - ds_sstep(0.86f, 1.0f, pass_u))
    );

    float s = pass_u * pass_u * (3.0f - 2.0f * pass_u);
    float decel = 1.0f - pow(1.0f - pass_u, 2.4f);
    float progress = mix(s, decel, 0.50f);
    float start_x = -half_width + 4.0f;
    float end_x = half_width + 18.0f;
    float head_x = mix(start_x, end_x, progress);

    float4 result = float4(0.0f);

    // Subtle traveling warm glow beneath the comet head.
    float glow_dx = (point.x - (head_x - 8.0f)) / 24.0f;
    float glow_dy = point.y / 6.5f;
    float glow_falloff = exp(-0.5f * (glow_dx * glow_dx + glow_dy * glow_dy)) * active_env;
    float point_fade = ds_sstep(0.0f, edge_fade, point.x + half_width)
        * ds_sstep(0.0f, edge_fade, half_width - point.x);
    if (glow_falloff > 0.005f) {
        ds_composite(result, mix(u.base.rgb, u.core.rgb, 0.45f), 0.16f * glow_falloff * point_fade);
    }

    float2 center_offset = float2(float(columns) - 1.0f, float(rows) - 1.0f) * 0.5f;
    int i0 = int(rint(point.x / pitch + center_offset.x));
    int j0 = int(rint(point.y / pitch + center_offset.y));
    float mid_row = (float(rows) - 1.0f) * 0.5f;

    for (int dj = -1; dj <= 1; dj++) {
        for (int di = -1; di <= 1; di++) {
            int i = i0 + di;
            int j = j0 + dj;
            if (i < 0 || j < 0 || i >= columns || j >= rows) {
                continue;
            }
            float2 center = (float2(float(i), float(j)) - center_offset) * pitch;
            float x_from_left = center.x + half_width;
            float mask = ds_sstep(0.0f, edge_fade, x_from_left)
                * ds_sstep(0.0f, edge_fade, width - x_from_left);
            if (mask <= 0.001f) {
                continue;
            }

            float row_dist = abs(float(j) - mid_row);
            float row_norm = row_dist / max(mid_row, 1.0f);
            float row_lag = row_dist * 1.55f * pitch;
            float delta = (head_x - row_lag) - center.x;

            float lead_sigma = 0.82f * pitch;
            float lead = exp(-(delta * delta) / (2.0f * lead_sigma * lead_sigma));
            float trail_len = max(pitch * 1.6f, (9.4f - 1.85f * row_dist) * pitch);
            float trail = exp(-max(delta, 0.0f) / trail_len);

            float wave = (delta < 0.0f ? lead : trail) * (1.0f - 0.14f * row_dist) * active_env;
            float power = clamp(wave, 1e-5f, 1.0f);
            float fresh = clamp((delta < 0.0f ? lead : exp(-delta / (2.3f * pitch))) * active_env, 0.0f, 1.0f);

            float floor_alpha = 0.095f * (1.0f - 0.50f * row_norm);
            float alpha = floor_alpha + (0.98f - floor_alpha) * pow(power, 0.85f * mix(1.30f, 1.0f, fresh));
            float dot_radius = (0.25f + 0.17f * pow(power, 0.6f)) * pitch;
            float3 color = mix(u.base.rgb, u.core.rgb, clamp(power * 1.05f, 0.0f, 0.9f) * mix(0.35f, 1.0f, fresh));
            float from_center = length(point - center);

            if (power > 0.06f) {
                ds_composite(
                    result,
                    color,
                    0.12f * power * power * mask * ds_coverage(from_center - dot_radius * 1.9f, aa)
                );
            }
            ds_composite(result, color, alpha * mask * ds_coverage(from_center - dot_radius, aa));
        }
    }

    result *= opacity;
    return ds_dithered(result, position);
}
