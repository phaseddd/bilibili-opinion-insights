use gpui::{FontWeight, IntoElement, ParentElement, Styled as _, Window, canvas, div, px};

use crate::gui::materials::{JellyMaterialToken, JellyTone};
use crate::gui::motion::{JellyMotionSnapshot, JellyProgressMotionSnapshot};
use crate::gui::rendering::jelly_geometry::{
    JellyPathShape, JellyRibbonChainShape, JellyRibbonShape, jelly_chained_ribbon,
    jelly_chained_ribbon_highlight, jelly_chained_ribbon_shadow, jelly_round_rect,
};
use crate::gui::theme::Palette;

#[derive(Clone, Copy)]
pub enum JellyProgressPhase {
    Idle,
    Validating,
    Running,
    Cancelling,
    Completed,
    Failed,
}

pub fn jelly_progress(
    motion: JellyProgressMotionSnapshot,
    phase: JellyProgressPhase,
    palette: &Palette,
) -> impl IntoElement {
    let percent = motion.display_percent.clamp(0., 100.);
    let target_percent = motion.target_percent.clamp(0., 100.);
    let fill = (percent / 100.).clamp(0.02, 1.);
    let token = progress_token(phase, palette);
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
    let microcopy = progress_microcopy(phase);
    let percent_label = if (target_percent - percent).abs() > 1.2 {
        format!("{percent:.0}% -> {target_percent:.0}%")
    } else {
        format!("{percent:.0}%")
    };
    let pulse = motion.pulse.clamp(0., 1.);
    let velocity_nudge = (motion.velocity * 4.).clamp(-5., 5.);

    gpui_component::v_flex()
        .gap(px(8.))
        .child(
            div()
                .relative()
                .h(px(46.))
                .w_full()
                .child(
                    canvas(
                        move |bounds, _window: &mut Window, _cx| JellyPathShape {
                            origin_x: f32::from(bounds.origin.x) + motion_snapshot.error_shake * 8.,
                            origin_y: f32::from(bounds.origin.y),
                            width: f32::from(bounds.size.width)
                                - motion_snapshot.error_shake.abs() * 16.,
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
                            let track_h = f32::from(bounds.size.height);
                            let track_w = f32::from(bounds.size.width);
                            let outer = jelly_round_rect(shape);
                            let shell_color = token.shell_start.opacity(0.12 + pulse * 0.08);
                            window.paint_path(outer, shell_color);

                            let shell_outline = jelly_round_rect(JellyPathShape {
                                origin_x,
                                origin_y,
                                width: track_w,
                                height: track_h,
                                inset: 0.,
                                inner_inset: 0.,
                                cap_taper: fill,
                                pressure: motion_snapshot.pressure,
                                rebound: motion_snapshot.rebound,
                                squash_x: motion_snapshot.squash_x,
                                squash_y: motion_snapshot.squash_y,
                            });
                            window.paint_path(
                                shell_outline,
                                token
                                    .rim
                                    .opacity(0.12 + motion_snapshot.rim_pressure * 0.14),
                            );

                            let ribbon_shape = JellyRibbonShape {
                                origin_x: origin_x + 3. + velocity_nudge,
                                origin_y: origin_y + 2.,
                                width: track_w - 6.,
                                height: track_h - 4.,
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
                            let ribbon_shadow = jelly_chained_ribbon_shadow(chained_shape);
                            window.paint_path(
                                ribbon_shadow,
                                token
                                    .contact_shadow
                                    .opacity(0.2 + motion_snapshot.contact * 0.09),
                            );

                            let ribbon = jelly_chained_ribbon(chained_shape);
                            window.paint_path(ribbon, token.shell_mid.opacity(0.76 + pulse * 0.14));

                            let ribbon_highlight = jelly_chained_ribbon_highlight(chained_shape);
                            window.paint_path(
                                ribbon_highlight,
                                token.specular.opacity(0.16 + pulse * 0.14),
                            );

                            let inner_width = (track_w * fill).max(track_h * 0.34 + 18.);
                            let inner_shape = JellyPathShape {
                                origin_x: origin_x + 7. + velocity_nudge * 0.32,
                                origin_y: origin_y + track_h * 0.2 + motion_snapshot.pressure * 1.2,
                                width: inner_width * (0.96 - motion_snapshot.squash_y * 0.04),
                                height: track_h * (0.58 - motion_snapshot.pressure * 0.04),
                                inset: 7.,
                                inner_inset: 5.,
                                cap_taper: (fill * 0.26).clamp(0., 1.),
                                pressure: motion_snapshot.pressure,
                                rebound: motion_snapshot.rebound,
                                squash_x: motion_snapshot.squash_x,
                                squash_y: motion_snapshot.squash_y * 0.54,
                            };
                            let fill_path = jelly_round_rect(inner_shape);
                            window
                                .paint_path(fill_path, token.core_top.opacity(0.44 + pulse * 0.12));
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(14.))
                        .top(px(8.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(token.text)
                        .child(percent_label),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(microcopy),
        )
}

fn progress_token(phase: JellyProgressPhase, palette: &Palette) -> JellyMaterialToken {
    JellyMaterialToken::for_tone(
        match phase {
            JellyProgressPhase::Idle => JellyTone::Neutral,
            JellyProgressPhase::Validating => JellyTone::Warning,
            JellyProgressPhase::Running => JellyTone::Primary,
            JellyProgressPhase::Cancelling => JellyTone::Warning,
            JellyProgressPhase::Completed => JellyTone::Success,
            JellyProgressPhase::Failed => JellyTone::Error,
        },
        palette,
    )
}

fn progress_microcopy(phase: JellyProgressPhase) -> &'static str {
    match phase {
        JellyProgressPhase::Idle => "等待开始采集。",
        JellyProgressPhase::Validating => "正在校验输入与登录态。",
        JellyProgressPhase::Running => "采集中：进度由真实事件推进。",
        JellyProgressPhase::Cancelling => "正在请求取消，等待 worker 返回。",
        JellyProgressPhase::Completed => "采集完成，结果可复核。",
        JellyProgressPhase::Failed => "采集失败，保留当前进度与错误信息。",
    }
}
