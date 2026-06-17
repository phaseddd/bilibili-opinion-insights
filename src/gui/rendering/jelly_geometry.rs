use gpui::{FillOptions, Path, PathBuilder, PathStyle, Pixels, point, px};

use crate::gui::motion::JellyProgressChainSnapshot;

const RIBBON_POINTS: usize = 9;
type RibbonPoint = (f32, f32);
type RibbonPoints = [RibbonPoint; RIBBON_POINTS];
type RibbonEdges = (RibbonPoints, RibbonPoints);
const RIBBON_EPSILON: f32 = 0.0001;

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyPathShape {
    pub(crate) origin_x: f32,
    pub(crate) origin_y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) inset: f32,
    pub(crate) inner_inset: f32,
    pub(crate) cap_taper: f32,
    pub(crate) pressure: f32,
    pub(crate) rebound: f32,
    pub(crate) squash_x: f32,
    pub(crate) squash_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonShape {
    pub(crate) origin_x: f32,
    pub(crate) origin_y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) progress: f32,
    pub(crate) pressure: f32,
    pub(crate) rebound: f32,
    pub(crate) compression: f32,
    pub(crate) phase: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonChainShape {
    pub(crate) shape: JellyRibbonShape,
    pub(crate) chain: JellyProgressChainSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonProfilePoint {
    pub(crate) center: RibbonPoint,
    pub(crate) normal: RibbonPoint,
    pub(crate) half_thickness: f32,
    pub(crate) progress_t: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonProfile {
    pub(crate) points: [JellyRibbonProfilePoint; RIBBON_POINTS],
}

// Reserved for the bitmap/GPU bridge; current GPUI path rendering only needs the profile edges.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct JellyRibbonSdfSample {
    pub(crate) signed_distance: f32,
    pub(crate) progress: f32,
    pub(crate) normal: RibbonPoint,
    pub(crate) inside: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct JellyRibbonAlphaMask {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) origin: RibbonPoint,
    pub(crate) pixel_size: f32,
    pub(crate) alpha: Vec<u8>,
    pub(crate) progress: Vec<u8>,
}

pub(crate) fn jelly_round_rect(shape: JellyPathShape) -> Path<Pixels> {
    let width = shape.width.max(8.);
    let height = shape.height.max(8.);
    let squash_x = shape.squash_x.clamp(0., 1.);
    let squash_y = shape.squash_y.clamp(0., 1.);
    let left = shape.origin_x + shape.inset + 2. - squash_x * 5.5;
    let right = shape.origin_x + width - shape.inset - 2. + squash_x * 5.5;
    let top = shape.origin_y + shape.inner_inset + 1.5 + squash_y * 1.8;
    let bottom = shape.origin_y + height - shape.inner_inset - 2.5 - squash_y * 2.6;
    let radius = (height * 0.42 + shape.pressure * 2.6).min(width * 0.24);
    let taper = shape.cap_taper.clamp(0., 1.);
    let inner_radius = (radius * (0.8 - taper * 0.08)).max(4.);

    let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::even_odd().with_tolerance(0.08),
    ));
    rounded_rect(&mut builder, left, top, right, bottom, radius);

    // Inner cavity to make the shell visibly thicker and leave more jelly exposed.
    let inner_left = left + 6. + taper * 2.5;
    let inner_right = right - 6. - taper * 2.5;
    let inner_top = top + 4. + shape.rebound * 0.9;
    let inner_bottom = bottom - 4.5 - shape.rebound * 0.8;
    rounded_rect(
        &mut builder,
        inner_left,
        inner_top,
        inner_right,
        inner_bottom,
        inner_radius,
    );

    builder
        .build()
        .unwrap_or_else(|_| Path::new(point(px(0.), px(0.))))
}

pub(crate) fn jelly_ribbon_profile(shape: JellyRibbonChainShape) -> JellyRibbonProfile {
    ribbon_profile_with_offsets(shape.shape, 0., 1., shape.chain.offsets)
}

#[allow(dead_code)]
pub(crate) fn sample_ribbon_sdf(
    profile: &JellyRibbonProfile,
    x: f32,
    y: f32,
) -> JellyRibbonSdfSample {
    let mut sample = JellyRibbonSdfSample {
        signed_distance: f32::INFINITY,
        progress: 0.,
        normal: (0., 1.),
        inside: false,
    };

    for idx in 0..RIBBON_POINTS - 1 {
        let start = profile.points[idx];
        let end = profile.points[idx + 1];
        let segment = (end.center.0 - start.center.0, end.center.1 - start.center.1);
        let len_sq = segment.0 * segment.0 + segment.1 * segment.1;
        let projection_t = if len_sq > RIBBON_EPSILON {
            (((x - start.center.0) * segment.0 + (y - start.center.1) * segment.1) / len_sq)
                .clamp(0., 1.)
        } else {
            0.
        };
        let center = (
            lerp(start.center.0, end.center.0, projection_t),
            lerp(start.center.1, end.center.1, projection_t),
        );
        let half_thickness = lerp(start.half_thickness, end.half_thickness, projection_t).max(0.);
        let delta = (x - center.0, y - center.1);
        let delta_len = (delta.0 * delta.0 + delta.1 * delta.1).sqrt();
        let signed_distance = delta_len - half_thickness;

        if signed_distance < sample.signed_distance {
            let normal = if delta_len > RIBBON_EPSILON {
                (delta.0 / delta_len, delta.1 / delta_len)
            } else {
                normalize((
                    lerp(start.normal.0, end.normal.0, projection_t),
                    lerp(start.normal.1, end.normal.1, projection_t),
                ))
            };
            sample = JellyRibbonSdfSample {
                signed_distance,
                progress: lerp(start.progress_t, end.progress_t, projection_t).clamp(0., 1.),
                normal,
                inside: signed_distance <= 0.,
            };
        }
    }

    if sample.signed_distance.is_finite() {
        sample
    } else {
        JellyRibbonSdfSample {
            signed_distance: 0.,
            progress: 0.,
            normal: (0., 1.),
            inside: true,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn rasterize_ribbon_alpha_mask(
    profile: &JellyRibbonProfile,
    pixel_size: f32,
    padding: f32,
) -> JellyRibbonAlphaMask {
    let pixel_size = pixel_size.max(0.25);
    let padding = padding.max(0.);
    let bounds = ribbon_profile_bounds(profile, padding);
    let width = (((bounds.2 - bounds.0) / pixel_size).ceil() as usize).max(1);
    let height = (((bounds.3 - bounds.1) / pixel_size).ceil() as usize).max(1);
    let mut alpha = vec![0; width * height];
    let mut progress = vec![0; width * height];

    for row in 0..height {
        let y = bounds.1 + (row as f32 + 0.5) * pixel_size;
        for col in 0..width {
            let x = bounds.0 + (col as f32 + 0.5) * pixel_size;
            let sample = sample_ribbon_sdf(profile, x, y);
            let edge_width = (pixel_size * 1.35).max(0.75);
            let coverage = (0.5 - sample.signed_distance / edge_width).clamp(0., 1.);
            let idx = row * width + col;
            alpha[idx] = (coverage * 255.).round() as u8;
            progress[idx] = (sample.progress * 255.).round() as u8;
        }
    }

    JellyRibbonAlphaMask {
        width,
        height,
        origin: (bounds.0, bounds.1),
        pixel_size,
        alpha,
        progress,
    }
}

pub(crate) fn jelly_chained_ribbon(shape: JellyRibbonChainShape) -> Path<Pixels> {
    let (top, bottom) = profile_edges(jelly_ribbon_profile(shape));
    closed_ribbon(top, bottom, 0.06)
}

pub(crate) fn jelly_chained_ribbon_highlight(shape: JellyRibbonChainShape) -> Path<Pixels> {
    let (top, _) = profile_edges(ribbon_profile_with_offsets(
        shape.shape,
        -shape.shape.height * 0.19,
        0.28,
        shape.chain.offsets,
    ));
    let mut bottom = top;
    for (idx, point) in bottom.iter_mut().enumerate() {
        let t = idx as f32 / (RIBBON_POINTS - 1) as f32;
        let taper = (std::f32::consts::PI * t).sin().max(0.).powf(0.5);
        point.1 += shape.shape.height * (0.11 + taper * 0.03);
        point.0 -= shape.shape.rebound.max(0.) * (1. - t) * 1.2;
    }
    closed_ribbon(top, bottom, 0.12)
}

pub(crate) fn jelly_chained_ribbon_shadow(shape: JellyRibbonChainShape) -> Path<Pixels> {
    let (mut top, mut bottom) = profile_edges(ribbon_profile_with_offsets(
        shape.shape,
        shape.shape.height * 0.2,
        0.78,
        shape.chain.offsets,
    ));
    for point in top.iter_mut().chain(bottom.iter_mut()) {
        point.1 += shape.shape.height * 0.18;
    }
    closed_ribbon(top, bottom, 0.2)
}

fn ribbon_profile_with_offsets(
    shape: JellyRibbonShape,
    y_offset: f32,
    thickness_scale: f32,
    chain_offsets: [f32; RIBBON_POINTS],
) -> JellyRibbonProfile {
    let progress = shape.progress.clamp(0.02, 1.);
    let width = shape.width.max(32.);
    let height = shape.height.max(12.);
    let pressure = shape.pressure.clamp(0., 1.);
    let rebound = shape.rebound.clamp(-1., 1.);
    let compression = shape.compression.clamp(0., 1.);
    let start_x = shape.origin_x + height * 0.48;
    let usable_w = width - height * 0.72;
    let end_x = start_x + (usable_w * progress).max(height * 0.68);
    let center_y = shape.origin_y + height * 0.5 + y_offset;
    let base_half = height * 0.28 * thickness_scale.max(0.08);
    let wave_phase = shape.phase;
    let active_amp = height * (0.1 + pressure * 0.18 + rebound.abs() * 0.07);
    let end_bulge = height * (0.1 + rebound.max(0.) * 0.16 + compression * 0.08);
    let mut centers = [(0., 0.); RIBBON_POINTS];
    let mut half_thicknesses = [0.; RIBBON_POINTS];

    for idx in 0..RIBBON_POINTS {
        let t = idx as f32 / (RIBBON_POINTS - 1) as f32;
        let sine = (std::f32::consts::PI * t).sin();
        let end_taper = 1. - (2. * (t - 0.5).abs()).clamp(0., 1.);
        let x = start_x + (end_x - start_x) * t;
        let local_wave = (wave_phase + t * std::f32::consts::PI * 1.6).sin();
        let arch = -sine * active_amp * (0.42 + compression * 0.58);
        let chain_arch = chain_offsets[idx].clamp(-0.5, 0.35) * height;
        let wobble = local_wave * (pressure + compression * 0.34) * height * 0.055 * end_taper;
        let tail_pull = rebound * (t - 0.5) * height * 0.14;
        let half = (base_half
            + sine * height * (0.05 + compression * 0.05) * thickness_scale
            + if idx == RIBBON_POINTS - 1 {
                end_bulge
            } else {
                0.
            })
        .max(2.5);
        let y = center_y + arch + chain_arch + wobble + tail_pull;
        let cap_push = if idx == RIBBON_POINTS - 1 {
            rebound.max(0.) * height * 0.2
        } else if idx == 0 {
            -rebound.max(0.) * height * 0.08
        } else {
            0.
        };
        centers[idx] = (x + cap_push, y);
        half_thicknesses[idx] = half;
    }

    let mut points = [JellyRibbonProfilePoint {
        center: (0., 0.),
        normal: (0., 1.),
        half_thickness: 0.,
        progress_t: 0.,
    }; RIBBON_POINTS];
    for idx in 0..RIBBON_POINTS {
        let tangent = if idx == 0 {
            (centers[1].0 - centers[0].0, centers[1].1 - centers[0].1)
        } else if idx == RIBBON_POINTS - 1 {
            (
                centers[idx].0 - centers[idx - 1].0,
                centers[idx].1 - centers[idx - 1].1,
            )
        } else {
            (
                centers[idx + 1].0 - centers[idx - 1].0,
                centers[idx + 1].1 - centers[idx - 1].1,
            )
        };
        let normal = normalize((-tangent.1, tangent.0));
        points[idx] = JellyRibbonProfilePoint {
            center: centers[idx],
            normal,
            half_thickness: half_thicknesses[idx],
            progress_t: idx as f32 / (RIBBON_POINTS - 1) as f32,
        };
    }

    JellyRibbonProfile { points }
}

fn profile_edges(profile: JellyRibbonProfile) -> RibbonEdges {
    let mut top = [(0., 0.); RIBBON_POINTS];
    let mut bottom = [(0., 0.); RIBBON_POINTS];
    for idx in 0..RIBBON_POINTS {
        let point = profile.points[idx];
        top[idx] = (
            point.center.0 - point.normal.0 * point.half_thickness,
            point.center.1 - point.normal.1 * point.half_thickness,
        );
        bottom[idx] = (
            point.center.0 + point.normal.0 * point.half_thickness,
            point.center.1 + point.normal.1 * point.half_thickness,
        );
    }
    (top, bottom)
}

fn ribbon_profile_bounds(profile: &JellyRibbonProfile, padding: f32) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for point in profile.points {
        let radius = point.half_thickness + padding;
        min_x = min_x.min(point.center.0 - radius);
        min_y = min_y.min(point.center.1 - radius);
        max_x = max_x.max(point.center.0 + radius);
        max_y = max_y.max(point.center.1 + radius);
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        (min_x, min_y, max_x, max_y)
    } else {
        (0., 0., 1., 1.)
    }
}

fn normalize(vector: RibbonPoint) -> RibbonPoint {
    let len = (vector.0 * vector.0 + vector.1 * vector.1).sqrt();
    if len > RIBBON_EPSILON {
        (vector.0 / len, vector.1 / len)
    } else {
        (0., 1.)
    }
}

fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t
}

fn closed_ribbon(top: RibbonPoints, bottom: RibbonPoints, tolerance: f32) -> Path<Pixels> {
    let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::non_zero().with_tolerance(tolerance),
    ));
    builder.move_to(point(px(top[0].0), px(top[0].1)));
    curve_through(&mut builder, &top);
    builder.line_to(point(
        px(bottom[RIBBON_POINTS - 1].0),
        px(bottom[RIBBON_POINTS - 1].1),
    ));
    let reversed_bottom = reverse_points(bottom);
    curve_through(&mut builder, &reversed_bottom);
    builder.close();
    builder
        .build()
        .unwrap_or_else(|_| Path::new(point(px(0.), px(0.))))
}

fn curve_through(builder: &mut PathBuilder, points: &RibbonPoints) {
    for idx in 1..RIBBON_POINTS {
        let prev = points[idx - 1];
        let next = points[idx];
        let ctrl = ((prev.0 + next.0) * 0.5, (prev.1 + next.1) * 0.5);
        builder.curve_to(point(px(next.0), px(next.1)), point(px(ctrl.0), px(ctrl.1)));
    }
}

fn reverse_points(points: RibbonPoints) -> RibbonPoints {
    let mut reversed = [(0., 0.); RIBBON_POINTS];
    for idx in 0..RIBBON_POINTS {
        reversed[idx] = points[RIBBON_POINTS - 1 - idx];
    }
    reversed
}

fn rounded_rect(
    builder: &mut PathBuilder,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) {
    let radius = radius.max(0.);
    let min_side = (right - left).abs().min((bottom - top).abs()) * 0.5;
    let radius = radius.min(min_side);
    let tl = point(px(left + radius), px(top));
    let tr = point(px(right - radius), px(top));
    let rr = point(px(right), px(top + radius));
    let br = point(px(right), px(bottom - radius));
    let bl = point(px(left + radius), px(bottom));
    let ll = point(px(left), px(bottom - radius));
    let lt = point(px(left), px(top + radius));

    builder.move_to(tl);
    builder.line_to(tr);
    builder.arc_to(point(px(radius), px(radius)), px(0.), false, true, rr);
    builder.line_to(br);
    builder.arc_to(
        point(px(radius), px(radius)),
        px(0.),
        false,
        true,
        point(px(right - radius), px(bottom)),
    );
    builder.line_to(bl);
    builder.arc_to(point(px(radius), px(radius)), px(0.), false, true, ll);
    builder.line_to(lt);
    builder.arc_to(point(px(radius), px(radius)), px(0.), false, true, tl);
    builder.close();
}

#[cfg(test)]
mod tests {
    use crate::gui::motion::JellyProgressChainSnapshot;

    use super::{
        JellyRibbonChainShape, JellyRibbonShape, RIBBON_POINTS, jelly_chained_ribbon,
        jelly_chained_ribbon_highlight, jelly_chained_ribbon_shadow, jelly_ribbon_profile,
        rasterize_ribbon_alpha_mask, sample_ribbon_sdf,
    };

    #[test]
    fn ribbon_paths_remain_finite_at_low_progress() {
        let shape = JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.02,
                pressure: 0.,
                rebound: 0.,
                compression: 0.,
                phase: 0.,
            },
            chain: JellyProgressChainSnapshot { offsets: [0.; 9] },
        };

        let _ = jelly_chained_ribbon(shape);
        let _ = jelly_chained_ribbon_highlight(shape);
        let _ = jelly_chained_ribbon_shadow(shape);
    }

    #[test]
    fn chained_ribbon_paths_remain_finite_with_offsets() {
        let shape = JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.34,
                pressure: 0.8,
                rebound: 0.4,
                compression: 0.7,
                phase: 1.2,
            },
            chain: JellyProgressChainSnapshot {
                offsets: [0., -0.04, -0.12, -0.16, -0.18, -0.12, -0.07, -0.03, 0.],
            },
        };

        let _ = jelly_chained_ribbon(shape);
        let _ = jelly_chained_ribbon_highlight(shape);
        let _ = jelly_chained_ribbon_shadow(shape);
    }

    #[test]
    fn ribbon_profile_points_remain_finite() {
        let shape = sample_shape();
        let profile = jelly_ribbon_profile(shape);

        for point in profile.points {
            assert!(point.center.0.is_finite());
            assert!(point.center.1.is_finite());
            assert!(point.normal.0.is_finite());
            assert!(point.normal.1.is_finite());
            assert!(point.half_thickness.is_finite());
            assert!(point.half_thickness > 0.);
            assert!((0.0..=1.0).contains(&point.progress_t));
        }
    }

    #[test]
    fn ribbon_profile_progress_is_monotonic() {
        let profile = jelly_ribbon_profile(sample_shape());

        for pair in profile.points.windows(2) {
            assert!(pair[0].progress_t <= pair[1].progress_t);
        }
    }

    #[test]
    fn ribbon_sdf_marks_centerline_inside() {
        let profile = jelly_ribbon_profile(sample_shape());
        let center = profile.points[RIBBON_POINTS / 2].center;
        let sample = sample_ribbon_sdf(&profile, center.0, center.1);

        assert!(sample.inside);
        assert!(sample.signed_distance <= 0.);
        assert!(sample.signed_distance.is_finite());
        assert!(sample.progress > 0.25);
        assert!(sample.progress < 0.75);
        assert!(sample.normal.0.is_finite());
        assert!(sample.normal.1.is_finite());
    }

    #[test]
    fn ribbon_sdf_marks_far_point_outside() {
        let profile = jelly_ribbon_profile(sample_shape());
        let sample = sample_ribbon_sdf(&profile, -300., -240.);

        assert!(!sample.inside);
        assert!(sample.signed_distance > 100.);
        assert!(sample.normal.0.is_finite());
        assert!(sample.normal.1.is_finite());
    }

    #[test]
    fn ribbon_profile_remains_finite_at_low_progress() {
        let shape = JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.001,
                pressure: 1.,
                rebound: -0.5,
                compression: 1.,
                phase: 8.1,
            },
            chain: JellyProgressChainSnapshot {
                offsets: [0.31, -0.5, -0.2, 0.2, 0.35, 0.1, -0.4, -0.1, 0.],
            },
        };
        let profile = jelly_ribbon_profile(shape);
        let sample = sample_ribbon_sdf(
            &profile,
            profile.points[0].center.0,
            profile.points[0].center.1,
        );

        for point in profile.points {
            assert!(point.center.0.is_finite());
            assert!(point.center.1.is_finite());
            assert!(point.normal.0.is_finite());
            assert!(point.normal.1.is_finite());
            assert!(point.half_thickness.is_finite());
            assert!(point.half_thickness > 0.);
        }
        assert!(sample.inside);
    }

    #[test]
    fn ribbon_alpha_mask_has_coverage_and_progress_gradient() {
        let profile = jelly_ribbon_profile(sample_shape());
        let mask = rasterize_ribbon_alpha_mask(&profile, 4., 10.);

        assert!(mask.width > 8);
        assert!(mask.height > 4);
        assert_eq!(mask.alpha.len(), mask.width * mask.height);
        assert_eq!(mask.progress.len(), mask.width * mask.height);
        assert!(mask.origin.0.is_finite());
        assert!(mask.origin.1.is_finite());
        assert!(mask.pixel_size > 0.);
        assert!(mask.alpha.iter().any(|alpha| *alpha > 220));
        assert!(mask.alpha.contains(&0));

        let covered_progress: Vec<u8> = mask
            .alpha
            .iter()
            .zip(mask.progress.iter())
            .filter_map(|(alpha, progress)| (*alpha > 160).then_some(*progress))
            .collect();
        let min_progress = covered_progress.iter().min().copied().unwrap_or(255);
        let max_progress = covered_progress.iter().max().copied().unwrap_or(0);

        assert!(min_progress < 80);
        assert!(max_progress > 170);
    }

    #[test]
    fn ribbon_alpha_mask_stays_bounded_at_low_progress() {
        let shape = JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.001,
                pressure: 0.9,
                rebound: 0.7,
                compression: 1.,
                phase: 2.4,
            },
            chain: JellyProgressChainSnapshot {
                offsets: [0.1, -0.12, -0.32, -0.5, -0.2, 0.12, 0.2, 0.1, 0.],
            },
        };
        let profile = jelly_ribbon_profile(shape);
        let mask = rasterize_ribbon_alpha_mask(&profile, 3., 8.);

        assert!(mask.width < 120);
        assert!(mask.height < 80);
        assert!(mask.alpha.iter().any(|alpha| *alpha > 0));
    }

    fn sample_shape() -> JellyRibbonChainShape {
        JellyRibbonChainShape {
            shape: JellyRibbonShape {
                origin_x: 10.,
                origin_y: 10.,
                width: 420.,
                height: 42.,
                progress: 0.58,
                pressure: 0.55,
                rebound: 0.36,
                compression: 0.7,
                phase: 1.2,
            },
            chain: JellyProgressChainSnapshot {
                offsets: [0., -0.04, -0.12, -0.16, -0.18, -0.12, -0.07, -0.03, 0.],
            },
        }
    }
}
