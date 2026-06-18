use gpui::{
    Bounds, Corners, FontWeight, IntoElement, ParentElement, SharedString, Styled as _, Window,
    canvas, div, linear_color_stop, linear_gradient, point, px, relative, size,
};
use gpui_component::{h_flex, v_flex};

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::{JellyMotionSnapshot, JellyProgressMotionSnapshot};
use crate::gui::rendering::jelly_geometry::{
    JellyPathShape, JellyRibbonChainShape, JellyRibbonShape, jelly_chained_ribbon,
    jelly_chained_ribbon_highlight, jelly_chained_ribbon_shadow, jelly_round_rect,
};
use crate::gui::rendering::jelly_image_cache::JellyProgressImage;
use crate::gui::state::events::EventKind;
use crate::gui::state::task::{TaskLane, TaskLanePhase};
use crate::gui::theme::Palette;
use crate::gui::views::primitives::status_badge;

pub(crate) fn jelly_task_lane(
    lane: &TaskLane,
    motion_tick: u64,
    palette: &Palette,
    bitmap: Option<JellyProgressImage>,
) -> impl IntoElement {
    let motion = lane.motion_snapshot(motion_tick);
    let material = lane_material(lane, palette);
    let phase_kind = lane_phase_kind(lane.phase);
    let title = format!("{} · {}", lane.bvid, lane.label());
    let detail = lane.detail();
    let percent_label = lane_percent_label(&motion);
    let pulse = motion.pulse.clamp(0., 1.);

    v_flex()
        .w_full()
        .gap(px(8.))
        .p(px(10.))
        .rounded(px(12.))
        .border_1()
        .border_color(material.rim.opacity(0.16 + motion.rim_pressure * 0.08))
        .bg(linear_gradient(
            135.,
            linear_color_stop(material.shell_start.opacity(0.05 + pulse * 0.02), 0.0),
            linear_color_stop(material.shell_end.opacity(0.08 + pulse * 0.03), 1.0),
        ))
        .shadow(vec![gpui::BoxShadow {
            color: material.state_aura.opacity(0.08 + motion.aura * 0.08),
            offset: gpui::point(px(0.), px(6.)),
            blur_radius: px(16.),
            spread_radius: px(-10.),
        }])
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(SharedString::from(title)),
                )
                .child(status_badge(lane.phase_label(), phase_kind, palette)),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(10.))
                .child(jelly_lane_ribbon(motion, material, bitmap))
                .child(
                    div()
                        .w(px(54.))
                        .text_align(gpui::TextAlign::Right)
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(material.text)
                        .child(percent_label),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .line_height(relative(1.25))
                .text_color(palette.muted)
                .child(SharedString::from(detail)),
        )
}

fn jelly_lane_ribbon(
    motion: JellyProgressMotionSnapshot,
    material: JellyMaterialToken,
    bitmap: Option<JellyProgressImage>,
) -> impl IntoElement {
    let fill = (motion.display_percent / 100.).clamp(0.03, 1.);
    let motion_snapshot = JellyMotionSnapshot {
        pressure: motion.pressure,
        rebound: motion.rebound,
        squash_x: motion.squash_x,
        squash_y: motion.squash_y,
        rim_pressure: motion.rim_pressure,
        gloss_phase: motion.gloss_phase,
        inner_lag: motion.inner_lag,
        contact: motion.contact,
        aura: motion.aura,
        error_shake: motion.error_shake,
    };
    let pulse = motion.pulse.clamp(0., 1.);
    let velocity_nudge = (motion.velocity * 3.).clamp(-4., 4.);

    div().relative().flex_1().min_w(px(120.)).h(px(26.)).child(
        canvas(
            move |bounds, _window: &mut Window, _cx| JellyPathShape {
                origin_x: f32::from(bounds.origin.x) + motion_snapshot.error_shake * 5.,
                origin_y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width) - motion_snapshot.error_shake.abs() * 10.,
                height: f32::from(bounds.size.height),
                inset: 0.,
                inner_inset: 0.,
                cap_taper: fill,
                pressure: motion_snapshot.pressure,
                rebound: motion_snapshot.rebound,
                squash_x: motion_snapshot.squash_x,
                squash_y: motion_snapshot.squash_y,
            },
            move |bounds, shape, window: &mut Window, _cx| {
                let origin_x = f32::from(bounds.origin.x);
                let origin_y = f32::from(bounds.origin.y);
                let track_w = f32::from(bounds.size.width);
                let track_h = f32::from(bounds.size.height);

                let shell = jelly_round_rect(shape);
                window.paint_path(shell, material.shell_start.opacity(0.1 + pulse * 0.06));

                let ribbon_shape = JellyRibbonShape {
                    origin_x: origin_x + 2. + velocity_nudge,
                    origin_y: origin_y + 1.,
                    width: track_w - 4.,
                    height: track_h - 2.,
                    progress: fill,
                    pressure: motion_snapshot.pressure,
                    rebound: motion_snapshot.rebound,
                    compression: motion_snapshot.contact,
                    phase: motion_snapshot.gloss_phase * std::f32::consts::TAU,
                };
                let chained_shape = JellyRibbonChainShape {
                    shape: ribbon_shape,
                    chain: motion.chain,
                };

                window.paint_path(
                    jelly_chained_ribbon_shadow(chained_shape),
                    material
                        .contact_shadow
                        .opacity(0.18 + motion_snapshot.contact * 0.08),
                );
                if let Some(bitmap) = bitmap.clone() {
                    let scale_x = (track_w - 4.) / bitmap.logical_width.max(1.);
                    let scale_y = track_h / bitmap.logical_height.max(1.);
                    let image_w = bitmap.width * scale_x;
                    let image_h = bitmap.height * scale_y;
                    let bitmap_bounds = Bounds::new(
                        point(
                            px(origin_x + 2. + velocity_nudge + bitmap.origin.0 * scale_x),
                            px(origin_y + bitmap.origin.1 * scale_y),
                        ),
                        size(px(image_w), px(image_h)),
                    );
                    let _ = window.paint_image(
                        bitmap_bounds,
                        Corners::from(px(image_h * 0.5)),
                        bitmap.image,
                        0,
                        false,
                    );
                } else {
                    window.paint_path(
                        jelly_chained_ribbon(chained_shape),
                        material.shell_mid.opacity(0.72 + pulse * 0.12),
                    );
                }
                window.paint_path(
                    jelly_chained_ribbon_highlight(chained_shape),
                    material.specular.opacity(0.06 + pulse * 0.05),
                );
            },
        )
        .absolute()
        .inset_0(),
    )
}

pub(crate) fn jelly_task_lane_tone(lane: &TaskLane) -> JellyTone {
    match lane.phase {
        TaskLanePhase::Pending => JellyTone::Neutral,
        TaskLanePhase::Discovering | TaskLanePhase::Cancelling => JellyTone::Warning,
        TaskLanePhase::Running => match lane.kind.event_kind() {
            EventKind::Danmaku => JellyTone::Cyan,
            _ => JellyTone::Primary,
        },
        TaskLanePhase::Completed => JellyTone::Success,
        TaskLanePhase::Failed => JellyTone::Error,
    }
}

fn lane_material(lane: &TaskLane, palette: &Palette) -> JellyMaterialToken {
    let tone = jelly_task_lane_tone(lane);
    JellyMaterialToken::for_tone(tone, palette)
}

fn lane_phase_kind(phase: TaskLanePhase) -> EventKind {
    match phase {
        TaskLanePhase::Pending => EventKind::System,
        TaskLanePhase::Discovering | TaskLanePhase::Cancelling => EventKind::Warning,
        TaskLanePhase::Running => EventKind::Video,
        TaskLanePhase::Completed => EventKind::Success,
        TaskLanePhase::Failed => EventKind::Failure,
    }
}

fn lane_percent_label(motion: &JellyProgressMotionSnapshot) -> String {
    let display = motion.display_percent.clamp(0., 100.);
    let target = motion.target_percent.clamp(0., 100.);
    if (display - target).abs() > 2. {
        format!("{display:.0}->{target:.0}%")
    } else {
        format!("{display:.0}%")
    }
}
