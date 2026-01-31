// Split Screen Compositor Shader
// Composites two camera textures with a dynamic diagonal split line

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var camera1_texture: texture_2d<f32>;
@group(2) @binding(1) var camera1_sampler: sampler;
@group(2) @binding(2) var camera2_texture: texture_2d<f32>;
@group(2) @binding(3) var camera2_sampler: sampler;
// split_params: x = angle, y = factor (0=unified, 1=split), z = center_x, w = center_y
@group(2) @binding(4) var<uniform> split_params: vec4<f32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let uv = mesh.uv;
    let angle = split_params.x;
    let factor = split_params.y;
    let center = vec2<f32>(split_params.z, split_params.w);

    // Sample both textures
    let color1 = textureSample(camera1_texture, camera1_sampler, uv);
    let color2 = textureSample(camera2_texture, camera2_sampler, uv);

    // If factor is 0 (unified mode), just show camera 1
    if (factor < 0.01) {
        return color1;
    }

    // Calculate which side of the split line this pixel is on
    // The split line passes through center with the given angle
    let pos = uv - center;

    // Normal to the split line (direction between players)
    let normal = vec2<f32>(cos(angle), sin(angle));

    // Signed distance from the split line
    let dist = dot(pos, normal);

    // Determine which camera to show based on side
    let side = select(0.0, 1.0, dist > 0.0);

    // Mix between unified (both show same) and split (each shows their side)
    let final_blend = mix(0.5, side, factor);
    var final_color = mix(color1, color2, final_blend);

    // Draw divider line
    let line_half_width = 0.003 * factor; // Line width scales with factor
    let abs_dist = abs(dist);

    if (abs_dist < line_half_width && factor > 0.1) {
        // White line with slight transparency
        let line_alpha = smoothstep(line_half_width, line_half_width * 0.5, abs_dist);
        let line_color = vec4<f32>(1.0, 1.0, 1.0, 0.9);
        final_color = mix(final_color, line_color, line_alpha * factor);
    }

    return final_color;
}
