use std::path::PathBuf;

use crate::app::collection::CollectionJobOutcome;
use crate::app::events::CollectionEvent;
use crate::bili::auth::{QrLoginSession, QrLoginStatus};
use crate::gui::state::auth::{AuthPhase, QrMatrix, SessionMode};
use crate::gui::state::results::FailureItem;

pub(crate) enum GuiMessage {
    Auth(AuthMessage),
    Event(CollectionEvent),
    Outcome(CollectionJobOutcome),
    Failure(FailureItem),
    UnitFinished,
    Finished { success: bool, message: String },
}

pub(crate) enum AuthMessage {
    BootChecking,
    NavChecked {
        phase: AuthPhase,
        session: SessionMode,
        message: String,
    },
    QrGenerated {
        session: QrLoginSession,
        matrix: QrMatrix,
        message: String,
    },
    QrStatus {
        status: QrLoginStatus,
        message: String,
    },
    QrCookieSaved {
        path: PathBuf,
        message: String,
    },
    QrNavRechecked {
        session: SessionMode,
        message: String,
    },
    AuthError {
        phase: AuthPhase,
        message: String,
    },
}
