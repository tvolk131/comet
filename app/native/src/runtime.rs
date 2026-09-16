//! Native services shared by storage, sync, and the iced UI.
use crate::{
    adapters::{native::key_store::UnlockedNostrKeys, nostr::sync_manager::SyncManager},
    domain::sync::model::{SyncChangePayload, SyncState},
    error::AppError,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::broadcast;
#[derive(Debug, Clone)]
pub enum AppEvent {
    SyncStatus(SyncState),
    RemoteChange(SyncChangePayload),
    TagsChanged,
    SyncProgress,
}
#[derive(Clone)]
pub struct AppContext {
    pub config_dir: Arc<PathBuf>,
    pub sync_manager: SyncManager,
    pub unlocked_keys: Arc<UnlockedNostrKeys>,
    events: broadcast::Sender<AppEvent>,
}
impl AppContext {
    pub fn new(config_dir: PathBuf) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            config_dir: Arc::new(config_dir),
            sync_manager: SyncManager::new(),
            unlocked_keys: Arc::default(),
            events,
        }
    }
    pub fn discover() -> Result<Self, AppError> {
        let path = if let Some(path) = std::env::var_os("COMET_DATA_DIR") {
            PathBuf::from(path)
        } else {
            let identifier = if cfg!(debug_assertions) {
                "md.comet-alpha.dev"
            } else {
                "md.comet-alpha"
            };
            dirs::config_dir()
                .ok_or_else(|| AppError::custom("Cannot find your configuration directory"))?
                .join(identifier)
        };
        Ok(Self::new(path))
    }
    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }
    pub fn notify(&self, event: AppEvent) {
        let _ = self.events.send(event);
    }
}
