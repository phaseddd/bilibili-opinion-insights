use crate::gui::motion::jelly_rebound;

#[derive(Default)]
pub(crate) struct VisualState {
    pub(crate) motion_tick: u64,
    pub(crate) motion_loop_running: bool,
    pub(crate) button_rebound: Option<ButtonRebound>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ButtonMotionId {
    EnterWorkbench,
    QrLogin,
    RecheckAuth,
    Anonymous,
    HeaderClear,
    HeaderCancel,
    HeaderStart,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ButtonRebound {
    pub(crate) id: ButtonMotionId,
    pub(crate) started_tick: u64,
}

impl VisualState {
    pub(crate) fn button_rebound_amount(&self, id: ButtonMotionId) -> f32 {
        let Some(rebound) = self.button_rebound else {
            return 0.0;
        };
        if rebound.id != id {
            return 0.0;
        }

        let age = self.motion_tick.saturating_sub(rebound.started_tick);
        if age > 18 {
            return 0.0;
        }

        jelly_rebound(age, 18)
    }

    pub(crate) fn has_active_button_rebound(&self) -> bool {
        self.button_rebound
            .map(|rebound| self.motion_tick.saturating_sub(rebound.started_tick) <= 18)
            .unwrap_or(false)
    }
}
