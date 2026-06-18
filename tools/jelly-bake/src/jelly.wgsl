// spike 0-A: 最小验证 shader —— 全屏三角形 + 2D 圆角盒 SDF 渐变。
// 目的只是证明 headless wgpu 管线（pipeline → render pass → readback → PNG）通。
// 0-B 会把 fragment 换成真正的 3D raymarch 厚胶胶体。

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
    // 覆盖全屏的大三角形
    var pts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(pts[idx], 0.0, 1.0);
}

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let res = vec2<f32>(512.0, 512.0);
    // 像素坐标 → 居中归一化 [-1,1]，y 向上
    var uv = (frag.xy / res) * 2.0 - vec2<f32>(1.0);
    uv.y = -uv.y;

    let d = sd_round_box(uv, vec2<f32>(0.62, 0.34), 0.30);

    // 背景浅冷灰
    var col = vec3<f32>(0.93, 0.94, 0.96);
    // 胶体填充（青蓝渐变）+ 柔和边缘
    let fill = mix(vec3<f32>(0.10, 0.62, 0.95), vec3<f32>(0.05, 0.42, 0.86), uv.y * 0.5 + 0.5);
    let inside = smoothstep(0.008, -0.008, d);
    col = mix(col, fill, inside);
    // 简单接触阴影
    let shadow = smoothstep(0.18, 0.0, d) * (1.0 - inside) * 0.25;
    col = col - vec3<f32>(shadow);

    return vec4<f32>(col, 1.0);
}
