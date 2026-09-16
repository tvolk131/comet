use crate::db::{
    active_account, active_account_attachments_dir, active_account_dir, app_database_path,
};
use crate::error::AppError;
use crate::runtime::AppContext;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    version: String,
    app_database_path: String,
    account_path: String,
    database_path: String,
    attachments_path: String,
    themes_path: String,
    active_npub: String,
}

pub fn app_status(app: AppContext) -> Result<AppStatus, AppError> {
    let config_dir = app.config_dir.as_ref().clone();
    let app_database_path = app_database_path(&app)?;
    let account = active_account(&app)?;
    let account_path = active_account_dir(&app)?;
    let attachments_path = active_account_attachments_dir(&app)?;
    let themes_path = config_dir.join("themes");

    Ok(AppStatus {
        version: env!("CARGO_PKG_VERSION").into(),
        app_database_path: app_database_path.to_string_lossy().into_owned(),
        account_path: account_path.to_string_lossy().into_owned(),
        database_path: account.db_path.to_string_lossy().into_owned(),
        attachments_path: attachments_path.to_string_lossy().into_owned(),
        themes_path: themes_path.to_string_lossy().into_owned(),
        active_npub: account.npub,
    })
}

pub fn get_attachments_dir(app: AppContext) -> Result<String, AppError> {
    crate::adapters::filesystem::attachments::get_attachments_dir(&app)
}

pub fn import_image(
    app: AppContext,
    source_path: String,
) -> Result<crate::adapters::filesystem::attachments::ImportedImage, AppError> {
    crate::adapters::filesystem::attachments::import_image(&app, &source_path)
}

pub fn import_image_bytes(
    app: AppContext,
    bytes: Vec<u8>,
) -> Result<crate::adapters::filesystem::attachments::ImportedImage, AppError> {
    crate::adapters::filesystem::attachments::import_image_bytes(&app, &bytes)
}

pub fn list_themes(app: AppContext) -> Result<Vec<crate::infra::themes::ThemeSummary>, AppError> {
    crate::infra::themes::list_themes(&app)
}

pub fn read_theme(
    app: AppContext,
    theme_id: String,
) -> Result<crate::infra::themes::ThemeData, AppError> {
    crate::infra::themes::read_theme(&app, &theme_id)
}

pub fn get_tag_index_diagnostics(
    app: AppContext,
) -> Result<crate::adapters::sqlite::tag_index::TagIndexDiagnostics, AppError> {
    let conn = crate::db::database_connection(&app)?;
    crate::adapters::sqlite::tag_index::tag_index_diagnostics(&conn)
}

pub fn repair_tag_index(
    app: AppContext,
) -> Result<crate::adapters::sqlite::tag_index::TagIndexDiagnostics, AppError> {
    let mut conn = crate::db::database_connection(&app)?;
    crate::adapters::sqlite::tag_index::repair_tag_index(&mut conn)
}
