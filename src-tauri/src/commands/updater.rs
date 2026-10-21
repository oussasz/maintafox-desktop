//! Updater IPC commands (master plan Phase 5.5; the logic is in `crate::updater`).
//!
//! `check_for_update`           — no session: safe at startup and on the login screen; never fails
//!                                the app (the result says why there is no update).
//! `install_pending_update`     — an administrator of this PC with a fresh identity check
//!                                (`install_authority`), for the offer they confirmed
//!                                (`ConfirmedOffer`): download with progress and signature check,
//!                                then pre-update backup, journal and install (the app restarts).
//! `reinstall_previous_version` — recovery screen after a failed database update (no session:
//!                                the database could not be opened by this version); only on the
//!                                version that failed, for exactly the version it came from.
//!
//! Only one install or reinstall runs at a time (`updater::InstallSlot`).
//! `get_update_journal` / `acknowledge_update_notice` — the outcome of the last update.

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::state::AppState;
use crate::updater::{
    ConfirmedOffer, InstallOutcome, InstallSlot, UpdateCheckResult, UpdateJournalView, UpdateProgressEvent,
};

fn database(app: &AppHandle) -> AppResult<sea_orm::DatabaseConnection> {
    app.try_state::<AppState>()
        .map(|s| s.db.clone())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("the database is not open yet")))
}

/// Asks the control API's manifest whether this device should install another version.
#[tauri::command]
pub async fn check_for_update(app: AppHandle, locale: Option<String>) -> AppResult<UpdateCheckResult> {
    let db = database(&app)?;
    Ok(crate::updater::check(&app, &db, locale.as_deref()).await)
}

/// Who may install an update on this PC: the administrator of the PC as the licence rule names it
/// (`product_license::licence_caller`: `adm.settings` plant-wide, or at any scope on a plant whose
/// administrators are all site-scoped), with a fresh identity check, as for a restore (an update
/// takes a backup, may stage a restore, and restarts the app for everyone on this PC).
///
/// It is the RBAC rule alone, not the licence permission matrix of `require_permission!`: a licence
/// that requires an update blocks everything else, and installing the update is how it ends, so
/// the install must stay possible then. It grants nothing the licence denies: what is installed is
/// what the update server offers this device, signed with the key built into this app. E-1: there
/// is no built-in administrator; the plant always has a named one (last-administrator guard) or
/// gets one with a vendor recovery code.
pub(crate) fn install_authority(caller: &crate::commands::product_license::LicenceCaller) -> AppResult<()> {
    if !caller.may_administer {
        return Err(AppError::PermissionDenied(format!(
            "Permission requise : {}",
            crate::rbac::permissions::ADM_SETTINGS
        )));
    }
    if !caller.step_up {
        return Err(AppError::StepUpRequired);
    }
    Ok(())
}

/// Installs the offer the administrator confirmed (`offer`: the version, kind and release of the
/// last check they saw), if the manifest still offers exactly that; otherwise `update_withdrawn`
/// and the UI shows the new offer for a new confirmation.
#[tauri::command]
pub async fn install_pending_update(
    app: AppHandle,
    state: State<'_, AppState>,
    locale: Option<String>,
    offer: ConfirmedOffer,
    on_progress: Channel<UpdateProgressEvent>,
) -> AppResult<InstallOutcome> {
    let user = require_session!(state);
    let step_up = state.session.read().await.is_step_up_valid();
    let caller = crate::commands::product_license::licence_caller(&state.db, Some(user.user_id), step_up).await?;
    install_authority(&caller)?;
    let _slot = InstallSlot::acquire()?;
    let progress = move |event: UpdateProgressEvent| {
        if let Err(e) = on_progress.send(event) {
            tracing::warn!("update progress could not reach the window: {e}");
        }
    };
    crate::updater::install(
        &app,
        &state.db,
        locale.as_deref(),
        Some(user.user_id),
        &offer,
        &progress,
    )
    .await
}

/// Reinstalls the version this device ran before a failed database update.
#[tauri::command]
pub async fn reinstall_previous_version(
    app: AppHandle,
    locale: Option<String>,
    on_progress: Channel<UpdateProgressEvent>,
) -> AppResult<InstallOutcome> {
    let db = database(&app)?;
    let _slot = InstallSlot::acquire()?;
    let progress = move |event: UpdateProgressEvent| {
        if let Err(e) = on_progress.send(event) {
            tracing::warn!("update progress could not reach the window: {e}");
        }
    };
    crate::updater::reinstall_previous(&app, &db, locale.as_deref(), &progress).await
}

/// The last update's journal (none when this device never updated in the app).
#[tauri::command]
pub fn get_update_journal(app: AppHandle) -> AppResult<Option<UpdateJournalView>> {
    crate::updater::journal_view(&app)
}

/// The user has read the outcome notice of the last update.
#[tauri::command]
pub fn acknowledge_update_notice(app: AppHandle) -> AppResult<()> {
    crate::updater::acknowledge_notice(&app)
}

#[cfg(test)]
mod tests {
    use super::install_authority;
    use crate::commands::product_license::LicenceCaller;
    use crate::errors::AppError;

    fn caller(may_administer: bool, step_up: bool) -> LicenceCaller {
        LicenceCaller {
            user_id: Some(7),
            may_administer,
            step_up,
        }
    }

    #[test]
    fn only_an_administrator_with_a_fresh_identity_check_installs() {
        assert!(install_authority(&caller(true, true)).is_ok());
        assert!(matches!(
            install_authority(&caller(true, false)),
            Err(AppError::StepUpRequired)
        ));
        match install_authority(&caller(false, true)) {
            Err(AppError::PermissionDenied(message)) => {
                assert!(message.contains(crate::rbac::permissions::ADM_SETTINGS), "{message}");
            }
            other => panic!("a user who does not administer this PC is refused: {other:?}"),
        }
        assert!(matches!(
            install_authority(&LicenceCaller::default()),
            Err(AppError::PermissionDenied(_))
        ));
    }
}
