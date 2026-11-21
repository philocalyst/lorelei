fn fill(input: FillInput) -> vec4<f32> {
    let uv = input.p / input.size;
    let phase = uv.x * 6.2831853 + input.time * 2.5;
    return vec4<f32>(0.35 + 0.3 * sin(phase), 0.55, 0.85, 0.36);
}
