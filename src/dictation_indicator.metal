#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float2 resolution;
    float time;
    float width;
    float height;
    float opacity;
    float scale;
    float softness;
    float average;
    float peak;
    float processing;
    float completion;
};

struct VertexOutput {
    float4 position [[position]];
    float2 uv;
};

vertex VertexOutput indicator_vertex(uint vertex_id [[vertex_id]]) {
    const float2 positions[] = {
        float2(-1.0, -1.0),
        float2( 3.0, -1.0),
        float2(-1.0,  3.0),
    };
    VertexOutput output;
    output.position = float4(positions[vertex_id], 0.0, 1.0);
    output.uv = positions[vertex_id] * float2(0.5, -0.5) + 0.5;
    return output;
}

float rounded_box(float2 point, float2 half_size, float radius) {
    float2 q = abs(point) - half_size + radius;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

float coverage(float distance, float softness) {
    return smoothstep(softness, -softness, distance);
}

float glow(float distance, float radius) {
    float outside = max(distance, 0.0);
    return exp2(-outside * outside / max(radius * radius, 0.001));
}

void composite(thread float4 &destination, float3 color, float alpha) {
    alpha = clamp(alpha, 0.0, 1.0);
    destination.rgb = color * alpha + destination.rgb * (1.0 - alpha);
    destination.a = alpha + destination.a * (1.0 - alpha);
}

void screen(thread float4 &destination, float3 color, float amount) {
    amount = clamp(amount, 0.0, 1.0);
    destination.rgb = 1.0 - (1.0 - destination.rgb) * (1.0 - color * amount);
}

fragment float4 indicator_fragment(
    VertexOutput input [[stage_in]],
    constant Uniforms &uniforms [[buffer(0)]])
{
    float backing_scale = uniforms.resolution.x / 112.0;
    float2 point = (input.uv * uniforms.resolution - uniforms.resolution * 0.5)
        / backing_scale / max(uniforms.scale, 0.001);
    float2 half_size = float2(uniforms.width, uniforms.height) * 0.5;
    float radius = uniforms.height * 0.5;
    float distance = rounded_box(point, half_size, radius);
    float lifecycle_softness = max(uniforms.softness, 0.0);
    float edge = (0.55 + lifecycle_softness * 0.72) / max(uniforms.scale, 0.001);
    float shape = coverage(distance, edge);
    float detail_clarity = exp2(-lifecycle_softness * 0.34);
    float4 result = 0.0;

    float processing = clamp(uniforms.processing, 0.0, 1.0);
    float completion = clamp(uniforms.completion, 0.0, 1.0);
    float completion_flash = smoothstep(0.0, 0.16, completion)
        * (1.0 - smoothstep(0.32, 1.0, completion));
    float recording = 1.0 - processing;
    float circularity = 1.0 - smoothstep(0.5, 2.25, abs(uniforms.width - uniforms.height));

    float3 red_accent = float3(1.0, 0.025, 0.035);
    float3 blue_accent = float3(0.1, 0.34, 1.0);
    float3 accent = mix(red_accent, blue_accent, processing);

    float average_power = clamp(uniforms.average, 0.0, 1.0);
    float peak_power = clamp(uniforms.peak, 0.0, 1.0);
    float average_activation = smoothstep(0.0, 0.1, average_power);
    float outer = recording * (
        glow(distance, 4.0 + lifecycle_softness * 0.5) * average_power * 0.72
        + glow(distance, 8.0 + lifecycle_softness) * average_power * 0.36);
    outer += processing * glow(distance, 6.0 + lifecycle_softness) * 0.13;
    outer += glow(distance, 10.0 + lifecycle_softness) * completion_flash * 0.32;
    composite(result, accent, outer * (1.0 - shape));

    float3 recording_base = mix(
        float3(0.5, 0.0, 0.0),
        float3(1.0, 0.0, 0.0),
        average_power);
    float sphere_light = clamp(0.48 - point.x * 0.035 - point.y * 0.045, 0.0, 1.0);
    float3 blue_base = mix(
        float3(0.0, 0.02, 0.32),
        float3(0.04, 0.18, 0.68),
        sphere_light);
    float3 base = mix(recording_base, blue_base, processing);
    composite(result, base, shape);

    float inside_edge = clamp(-distance / 4.0, 0.0, 1.0);
    float inner_edge = shape * (1.0 - inside_edge);
    float inner_edge_strength = mix(0.48, 0.4, processing);
    composite(result, accent, inner_edge * inner_edge_strength);

    // Match the original Hex recording stack: a padded red fill, a nearly
    // full-width white beam, and a broad peak-driven red glow.
    float red_fill_distance = rounded_box(point, float2(22.0, 2.0), 2.0);
    float red_fill = exp2(-pow(max(red_fill_distance, 0.0) / 2.0, 2.0)) * shape;
    screen(result, float3(1.0, 0.0, 0.0), red_fill * average_activation * recording * detail_clarity);

    float white_beam_distance = rounded_box(point, float2(21.0, 1.0), 1.0);
    float white_beam = exp2(-pow(max(white_beam_distance, 0.0) / 1.0, 2.0)) * shape;
    screen(
        result,
        float3(1.0),
        white_beam * average_activation * 0.56 * recording * detail_clarity);

    float peak_half_width = 22.0 * min(peak_power + 0.6, 1.0);
    float peak_distance = rounded_box(point, float2(peak_half_width, 2.0), 2.0);
    float peak_beam = exp2(-pow(max(peak_distance, 0.0) / 4.0, 2.0)) * shape;
    screen(
        result,
        float3(1.0, 0.0, 0.0),
        peak_beam * smoothstep(0.0, 0.1, peak_power) * 0.5 * recording * detail_clarity);

    if (processing > 0.0) {
        float orb_radius = uniforms.height * 0.5 - 0.6;
        float orb_distance = length(point) - orb_radius;
        float orb_mask = coverage(orb_distance, edge) * circularity;
        float charged_center = exp2(-dot(point, point) / 32.0) * 0.29;
        composite(result, blue_accent, charged_center * orb_mask * processing * detail_clarity);

        float radial = length(point);
        float rim = exp2(-pow((radial - orb_radius + 1.25) / 0.7, 2.0));
        composite(result, float3(0.35, 0.65, 1.0), rim * orb_mask * processing * 0.36 * detail_clarity);

        // Kinograph-style processing energy: fixed geometry with one broad light
        // band sweeping through the clipped surface, never orbiting or breathing.
        float sweep_position = point.x + uniforms.width * 0.5;
        float sweep_travel = uniforms.width + 70.0;
        float sweep_start = fmod(uniforms.time * 110.0, sweep_travel) - 8.0;
        float sweep = 0.0;
        for (int index = 0; index < 3; index++) {
            float band_center = sweep_start - float(index) * 27.0;
            float band_distance = abs(sweep_position - band_center);
            float band = 1.0 - smoothstep(0.25, 8.0, band_distance);
            sweep = max(sweep, band);
        }
        float sweep_reveal = smoothstep(0.58, 0.92, processing) * circularity;
        composite(
            result,
            float3(0.7, 0.87, 1.0),
            sweep * orb_mask * sweep_reveal * (1.0 - smoothstep(0.0, 0.5, completion))
                * 0.68 * detail_clarity);
    }

    screen(result, float3(0.72, 0.88, 1.0), shape * completion_flash * 0.42);

    float stroke = coverage(
        abs(distance) - 0.5,
        0.45 + lifecycle_softness * 0.3) * 0.56 * detail_clarity;
    composite(result, mix(accent, float3(1.0), 0.1), stroke);

    result *= uniforms.opacity;
    return result;
}
