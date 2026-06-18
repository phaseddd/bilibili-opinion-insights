// spike 0-B: 厚胶按钮 raymarch。
// 移植自 jelly-switch（cerpow / Voicu Apostol）的 TypeGPU shader 思路，
// 以 Rust-native WGSL 重写：圆角盒胶体 SDF + Fresnel/折射/Beer-Lambert/forward scatter/AO。
// 不是 TS 复制；交互/音效/TAA 全部去掉，离线固定机位烤一帧。

struct Uniforms {
    view_inv: mat4x4<f32>,
    proj_inv: mat4x4<f32>,
    light_dir: vec4<f32>,   // xyz = 光传播方向（已归一化）
    jelly_color: vec4<f32>,
    progress: f32,
    squash_x: f32,
    squash_y: f32,
    squash_z: f32,
    wiggle_x: f32,
    exposure: f32,
    resolution: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// ---- 常数（jelly-switch/constants.ts）----
const MAX_STEPS: i32 = 64;
const MAX_DIST: f32 = 10.0;
const SURF_DIST: f32 = 0.001;
const JELLY_IOR: f32 = 1.42;
const JELLY_SCATTER_STRENGTH: f32 = 3.0;
const SPECULAR_POWER: f32 = 10.0;
const SPECULAR_INTENSITY: f32 = 0.6;
const AMBIENT_COLOR: f32 = 0.6;
const AMBIENT_INTENSITY: f32 = 0.6;
const AO_STEPS: i32 = 3;
const AO_RADIUS: f32 = 0.1;
const AO_INTENSITY: f32 = 0.5;
const AO_BIAS: f32 = SURF_DIST * 5.0;
const JELLY_HALFSIZE: vec3<f32> = vec3<f32>(1.7, 0.4, 0.55);
const SWITCH_RAIL_LENGTH: f32 = 0.4;
const GROUND_THICKNESS: f32 = 0.03;
const GROUND_ROUNDNESS: f32 = 0.02;
const GROUND_RADIUS: f32 = 0.05;

// ---- SDF 图元（Inigo Quilez 标准公式）----
fn sd_rounded_box3d(p: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec3<f32>(r);
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}
fn sd_rounded_box2d(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
fn sd_plane(p: vec3<f32>, n: vec3<f32>, h: f32) -> f32 {
    return dot(p, n) + h;
}
fn op_extrude_y(p: vec3<f32>, d2d: f32, h: f32) -> f32 {
    let w = vec2<f32>(d2d, abs(p.y) - h);
    return min(max(w.x, w.y), 0.0) + length(max(w, vec2<f32>(0.0)));
}

// ---- 形变算子 ----
fn op_cheap_bend(p: vec3<f32>, k: f32) -> vec3<f32> {
    let c = cos(k * p.x);
    let s = sin(k * p.x);
    let m = mat2x2<f32>(c, -s, s, c);
    return vec3<f32>(m * p.xy, p.z);
}
fn op_rotate_axis_angle(p: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    return mix(axis * dot(p, axis), p, vec3<f32>(cos(angle))) + cross(p, axis) * sin(angle);
}

// ---- 场景 SDF ----
fn rectangle_cutout_dist(p: vec2<f32>) -> f32 {
    let b = vec2<f32>(SWITCH_RAIL_LENGTH * 0.5 + 0.2 + GROUND_ROUNDNESS, GROUND_RADIUS + GROUND_ROUNDNESS);
    return sd_rounded_box2d(p, b, GROUND_RADIUS + GROUND_ROUNDNESS);
}
fn main_scene_dist(p: vec3<f32>) -> f32 {
    // 按钮版：去掉 switch 轨道凹槽，只留地面平面
    return sd_plane(p, vec3<f32>(0.0, 1.0, 0.0), 0.06);
}
// 按钮版：胶体居中（不像 switch 沿轨道滑），保留 squash/wiggle 形变接口
fn jelly_dist(p: vec3<f32>) -> f32 {
    let jelly_origin = vec3<f32>(0.0, JELLY_HALFSIZE.y * 0.5, 0.0);
    let inv_scale = vec3<f32>(1.0 - u.squash_x, 1.0 + u.squash_y, 1.0 - u.squash_z);
    let local0 = (p - jelly_origin) * inv_scale;
    let local = op_rotate_axis_angle(local0, vec3<f32>(0.0, 0.0, 1.0), u.wiggle_x);
    return sd_rounded_box3d(op_cheap_bend(local, 0.0), JELLY_HALFSIZE - vec3<f32>(0.1), 0.1);
}

struct Hit { dist: f32, is_jelly: bool };
fn scene_dist(p: vec3<f32>) -> Hit {
    let m = main_scene_dist(p);
    let j = jelly_dist(p);
    var h: Hit;
    if (j < m) { h.dist = j; h.is_jelly = true; }
    else { h.dist = m; h.is_jelly = false; }
    return h;
}
fn scene_dist_ao(p: vec3<f32>) -> f32 {
    return min(main_scene_dist(p), jelly_dist(p));
}
fn approx_normal(p: vec3<f32>, e: f32) -> vec3<f32> {
    let d = scene_dist(p).dist;
    let n = vec3<f32>(
        scene_dist(p + vec3<f32>(e, 0.0, 0.0)).dist - d,
        scene_dist(p + vec3<f32>(0.0, e, 0.0)).dist - d,
        scene_dist(p + vec3<f32>(0.0, 0.0, e)).dist - d,
    );
    return normalize(n);
}
fn get_normal(p: vec3<f32>) -> vec3<f32> {
    if (abs(p.z) > 0.7 || abs(p.x) > 1.95) { return vec3<f32>(0.0, 1.0, 0.0); }
    return approx_normal(p, 0.0001);
}

// ---- 材质 ----
fn fresnel_schlick(cos_theta: f32, ior1: f32, ior2: f32) -> f32 {
    let r0 = pow((ior1 - ior2) / (ior1 + ior2), 2.0);
    return r0 + (1.0 - r0) * pow(1.0 - cos_theta, 5.0);
}
fn beer_lambert(sigma: vec3<f32>, dist: f32) -> vec3<f32> {
    return exp(sigma * (-dist));
}
fn fake_shadow(p: vec3<f32>, light_dir: vec3<f32>) -> vec3<f32> {
    // 按钮版接触阴影：胶体正下方（水平距中心近）压暗地面
    let r = length(p.xz);
    let contact = clamp(r * 2.0 - 0.15, 0.0, 1.0);
    return vec3<f32>(mix(0.4, 1.0, contact));
}
fn calc_ao(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var total = 0.0;
    var w = 1.0;
    let step = AO_RADIUS / f32(AO_STEPS);
    for (var i = 1; i <= AO_STEPS; i = i + 1) {
        let h = step * f32(i);
        let d = scene_dist_ao(p + n * h) - AO_BIAS;
        total = total + max(0.0, h - d) * w;
        w = w * 0.5;
        if (total > AO_RADIUS / AO_INTENSITY) { break; }
    }
    return clamp(1.0 - (AO_INTENSITY * total) / AO_RADIUS, 0.0, 1.0);
}
fn calc_lighting(hit_pos: vec3<f32>, n: vec3<f32>, ray_origin: vec3<f32>) -> vec3<f32> {
    let light_dir = -u.light_dir.xyz;
    let shadow = fake_shadow(hit_pos, light_dir);
    let diffuse = max(dot(n, light_dir), 0.0);
    let view_dir = normalize(ray_origin - hit_pos);
    let reflect_dir = reflect(-light_dir, n);
    let spec_factor = pow(max(dot(view_dir, reflect_dir), 0.0), SPECULAR_POWER);
    let specular = vec3<f32>(spec_factor * SPECULAR_INTENSITY);
    let base = vec3<f32>(0.9);
    let directional = base * diffuse * shadow;
    let ambient = base * AMBIENT_COLOR * AMBIENT_INTENSITY;
    return clamp(directional + ambient + specular * shadow, vec3<f32>(0.0), vec3<f32>(1.0));
}
fn render_background(ray_origin: vec3<f32>, ray_dir: vec3<f32>, hit_dist: f32) -> vec3<f32> {
    let hit_pos = ray_origin + ray_dir * hit_dist;
    let n = get_normal(hit_pos);
    let jc = u.jelly_color.rgb;
    let d = hit_pos - vec3<f32>(0.0, 0.0, 0.0);
    let sq_dist = dot(d, d);
    let bounce = jc * ((1.0 / (sq_dist * 15.0 + 1.0)) * 0.4);
    let side_bounce = jc * ((1.0 / (sq_dist * 40.0 + 1.0)) * 0.3) * abs(n.z);
    let emission = smoothstep(0.7, 1.0, u.progress) * 2.0 + 0.7;
    let lit = calc_lighting(hit_pos, n, ray_origin);
    let ao = calc_ao(hit_pos, n);
    return vec3<f32>(1.0) * lit * ao + bounce * emission + side_bounce * emission;
}
fn raymarch_no_jelly(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> vec3<f32> {
    var dist = 0.0;
    for (var i = 0; i < 6; i = i + 1) {
        let h = main_scene_dist(ray_origin + ray_dir * dist);
        dist = dist + h;
        if (dist > MAX_DIST || h < SURF_DIST * 10.0) { break; }
    }
    if (dist < MAX_DIST) { return render_background(ray_origin, ray_dir, dist); }
    return vec3<f32>(0.0);
}
fn intersect_box(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec3<f32> {
    let inv = vec3<f32>(1.0) / rd;
    let t1 = (bmin - ro) * inv;
    let t2 = (bmax - ro) * inv;
    let tmin_v = min(t1, t2);
    let tmax_v = max(t1, t2);
    let tmin = max(max(tmin_v.x, tmin_v.y), tmin_v.z);
    let tmax = min(min(tmax_v.x, tmax_v.y), tmax_v.z);
    let hit = select(0.0, 1.0, tmax >= tmin && tmax >= 0.0);
    return vec3<f32>(hit, tmin, tmax);
}
fn raymarch(ray_origin: vec3<f32>, ray_dir: vec3<f32>) -> vec4<f32> {
    var bg_dist = 0.0;
    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        let h = main_scene_dist(ray_origin + ray_dir * bg_dist);
        bg_dist = bg_dist + h;
        if (h < SURF_DIST) { break; }
    }
    // 透明背景资产：地面/背景不输出（alpha=0），只保留胶体本体。
    // 折射环境采样仍用 raymarch_no_jelly（内部走 render_background）。
    let bb = intersect_box(ray_origin, ray_dir, vec3<f32>(-2.1, -1.0, -1.0), vec3<f32>(2.1, 1.0, 1.0));
    if (bb.x < 0.5) { return vec4<f32>(0.0); }

    var dist = max(0.0, bb.y);
    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        let cur = ray_origin + ray_dir * dist;
        let hit = scene_dist(cur);
        dist = dist + hit.dist;
        if (hit.dist < SURF_DIST) {
            let hit_pos = ray_origin + ray_dir * dist;
            if (!hit.is_jelly) { break; }
            let N = get_normal(hit_pos);
            let I = ray_dir;
            let cosi = min(1.0, max(0.0, dot(-I, N)));
            let F = fresnel_schlick(cosi, 1.0, JELLY_IOR);
            let reflection = clamp(vec3<f32>(hit_pos.y + 0.2), vec3<f32>(0.0), vec3<f32>(1.0));
            let eta = 1.0 / JELLY_IOR;
            let k = 1.0 - eta * eta * (1.0 - cosi * cosi);
            var refracted = vec3<f32>(0.0);
            if (k > 0.0) {
                let refr_dir = normalize(I * eta + N * (eta * cosi - sqrt(k)));
                let exit_pos = hit_pos + refr_dir * (SURF_DIST * 4.0);
                let env = raymarch_no_jelly(exit_pos, refr_dir);
                let jc = u.jelly_color.rgb;
                let scatter_tint = jc * 1.5;
                let absorb = (vec3<f32>(1.0) - jc) * 20.0;
                let prog = clamp(mix(1.0, 0.6, hit_pos.y * (1.0 / (JELLY_HALFSIZE.y * 2.0)) + 0.25), 0.0, 1.0) * u.progress;
                let t = beer_lambert(absorb * (prog * prog), 0.08);
                let forward = max(0.0, dot(-u.light_dir.xyz, refr_dir));
                let scatter = scatter_tint * (JELLY_SCATTER_STRENGTH * forward * (prog * prog * prog));
                refracted = env * t + scatter;
            }
            return vec4<f32>(reflection * F + refracted * (1.0 - F), 1.0);
        }
        if (dist > bg_dist || dist > MAX_DIST) { break; }
    }
    return vec4<f32>(0.0);
}

// ---- 入口 ----
struct Ray { origin: vec3<f32>, dir: vec3<f32> };
fn get_ray(ndc: vec2<f32>) -> Ray {
    let clip = vec4<f32>(ndc.x, ndc.y, -1.0, 1.0);
    let view_pos = u.proj_inv * clip;
    let view_pos_n = vec4<f32>(view_pos.xyz / view_pos.w, 1.0);
    let world = u.view_inv * view_pos_n;
    let origin = u.view_inv[3].xyz;
    return Ray(origin, normalize(world.xyz - origin));
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
    var pts = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(pts[idx], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let res = u.resolution;
    let uv = frag.xy / res;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, -(uv.y * 2.0 - 1.0));
    let ray = get_ray(ndc);
    let color = raymarch(ray.origin, ray.dir);
    return vec4<f32>(tanh(color.rgb * u.exposure), color.a);
}
