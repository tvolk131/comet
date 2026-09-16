use crate::db;
use crate::domain::accounts::model::{AccountSummary, SecretStorageStatus};
use crate::error::AppError;
use crate::runtime::AppContext;

async fn run_account_change<T>(
    app: &AppContext,
    change: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let manager = app.sync_manager.clone();
    manager.stop().await;

    let result = change();
    crate::adapters::nostr::sync_manager::auto_start(app).await;
    result
}

pub fn list_accounts(app: AppContext) -> Result<Vec<AccountSummary>, AppError> {
    db::list_accounts(&app)
}

pub fn get_account_nsec(app: AppContext, public_key: String) -> Result<String, AppError> {
    db::get_account_nsec(&app, &public_key)
}

pub fn get_secret_storage_status(app: AppContext) -> Result<SecretStorageStatus, AppError> {
    db::current_secret_storage_status(&app)
}

pub async fn move_secret_to_keychain(app: AppContext) -> Result<SecretStorageStatus, AppError> {
    run_account_change(&app, || db::move_current_account_nsec_to_keychain(&app)).await
}

pub async fn add_account(
    app: AppContext,
    nsec: String,
    store_in_keychain: bool,
) -> Result<AccountSummary, AppError> {
    run_account_change(&app, || db::add_account(&app, &nsec, store_in_keychain)).await
}

pub async fn switch_account(
    app: AppContext,
    public_key: String,
) -> Result<AccountSummary, AppError> {
    run_account_change(&app, || db::switch_account(&app, &public_key)).await
}

pub fn rename_account(app: AppContext, public_key: String, name: String) -> Result<(), AppError> {
    db::rename_account(&app, &public_key, &name)
}
