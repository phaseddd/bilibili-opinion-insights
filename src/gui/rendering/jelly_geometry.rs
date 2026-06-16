use gpui::{FillOptions, Path, PathBuilder, PathStyle, Pixels, point, px};

use crate::gui::motion::JellyProgressChainSnapshot;

const RIBBON_POINTS: usize = 9;
type RibbonPoint = (f32, f32);
type RibbonPoints = [RibbonPoint; RIBBON_POINTS];
type RibbonEdges = (RibbonPoints, RibbonPoints);

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

pub(crate) fn jelly_chained_ribbon(shape: JellyRibbonChainShape) -> Path<Pixels> {
    let (top, bottom) = chained_ribbon_edges(shape, 0., 1.);
    closed_ribbon(top, bottom, 0.06)
}

pub(crate) fn jelly_chained_ribbon_highlight(shape: JellyRibbonChainShape) -> Path<Pixels> {
    let (top, _) = chained_ribbon_edges(shape, -shape.shape.height * 0.19, 0.28);
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
    let (mut top, mut bottom) = chained_ribbon_edges(shape, shape.shape.height * 0.2, 0.78);
    for point in top.iter_mut().chain(bottom.iter_mut()) {
        point.1 += shape.shape.height * 0.18;
    }
    closed_ribbon(top, bottom, 0.2)
}

fn chained_ribbon_edges(
    shape: JellyRibbonChainShape,
    y_offset: f32,
    thickness_scale: f32,
) -> RibbonEdges {
    ribbon_edges_with_offsets(shape.shape, y_offset, thickness_scale, shape.chain.offsets)
}

fn ribbon_edges_with_offsets(
    shape: JellyRibbonShape,
    y_offset: f32,
    thickness_scale: f32,
    chain_offsets: [f32; RIBBON_POINTS],
) -> RibbonEdges {
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
    let active_amp = height * (0.08 + pressure * 0.14 + rebound.abs() * 0.05);
    let end_bulge = height * (0.08 + rebound.max(0.) * 0.12 + compression * 0.06);
    let mut top = [(0., 0.); RIBBON_POINTS];
    let mut bottom = [(0., 0.); RIBBON_POINTS];

    for idx in 0..RIBBON_POINTS {
        let t = idx as f32 / (RIBBON_POINTS - 1) as f32;
        let sine = (std::f32::consts::PI * t).sin();
        let end_taper = 1. - (2. * (t - 0.5).abs()).clamp(0., 1.);
        let x = start_x + (end_x - start_x) * t;
        let local_wave = (wave_phase + t * std::f32::consts::PI * 1.6).sin();
        let arch = -sine * active_amp * (0.42 + compression * 0.58);
        let chain_arch = chain_offsets[idx].clamp(-0.5, 0.35) * height;
        let wobble = local_wave * pressure * height * 0.035 * end_taper;
        let tail_pull = rebound * (t - 0.5) * height * 0.11;
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
            rebound.max(0.) * height * 0.16
        } else if idx == 0 {
            -rebound.max(0.) * height * 0.06
        } else {
            0.
        };
        top[idx] = (x + cap_push, y - half);
        bottom[idx] = (x + cap_push, y + half);
    }

    (top, bottom)
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
        JellyRibbonChainShape, JellyRibbonShape, jelly_chained_ribbon,
        jelly_chained_ribbon_highlight, jelly_chained_ribbon_shadow,
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
}
