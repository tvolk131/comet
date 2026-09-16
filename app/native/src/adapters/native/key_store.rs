use crate::error::AppError;
use crate::runtime::AppContext;
use keyring::Entry;
use nostr_sdk::prelude::*;
use rusqlite::{Connection, OptionalExtension};
use std::{collections::HashMap, sync::Mutex};

const NOSTR_NSEC_KEY_PREFIX: &str = "nostr-nsec";

#[derive(Default)]
pub struct UnlockedNostrKeys {
    keys_by_public_key: Mutex<HashMap<String, Keys>>,
}

// ---------------------------------------------------------------------------
// Implementation helpers
// ---------------------------------------------------------------------------

fn storage_key(public_key: &str) -> String {
    format!("{NOSTR_NSEC_KEY_PREFIX}:{public_key}")
}

fn entry(public_key: &str) -> Result<Entry, AppError> {
    // Keep the same OS keychain service and account names as existing installations.
    Entry::new("comet", &storage_key(public_key)).map_err(|e| plugin_error(e.to_string()))
}

fn plugin_error(message: impl Into<String>) -> AppError {
    AppError::custom(format!("Secure storage error: {}", message.into()))
}

fn current_identity_public_key(conn: &Connection) -> Result<String, AppError> {
    conn.query_row("SELECT public_key FROM nostr_identity LIMIT 1", [], |row| {
        row.get(0)
    })
    .optional()?
    .ok_or_else(|| AppError::custom("No Nostr identity configured."))
}

fn current_identity_db_nsec(conn: &Connection) -> Result<Option<String>, AppError> {
    conn.query_row("SELECT nsec FROM nostr_identity LIMIT 1", [], |row| {
        row.get::<_, Option<String>>(0)
    })
    .optional()
    .map(|value| value.flatten())
    .map_err(Into::into)
}

fn cache_state(app: &AppContext) -> &UnlockedNostrKeys {
    &app.unlocked_keys
}

fn cached_keys_for_account(app: &AppContext, public_key: &str) -> Result<Option<Keys>, AppError> {
    let cache = cache_state(app);
    let guard = cache
        .keys_by_public_key
        .lock()
        .map_err(|_| AppError::custom("Failed to access unlocked key cache."))?;
    Ok(guard.get(public_key).cloned())
}

fn cache_account_keys(app: &AppContext, public_key: &str, keys: &Keys) -> Result<(), AppError> {
    let cache = cache_state(app);
    let mut guard = cache
        .keys_by_public_key
        .lock()
        .map_err(|_| AppError::custom("Failed to update unlocked key cache."))?;
    guard.insert(public_key.to_string(), keys.clone());
    Ok(())
}

// ---------------------------------------------------------------------------
// Public free functions (backward-compatible API)
// ---------------------------------------------------------------------------

pub fn is_current_identity_unlocked(app: &AppContext, conn: &Connection) -> Result<bool, AppError> {
    let public_key = current_identity_public_key(conn)?;
    Ok(cached_keys_for_account(app, &public_key)?.is_some()
        || current_identity_db_nsec(conn)?.is_some())
}

pub fn store_account_nsec(
    app: &AppContext,
    public_key: &str,
    raw_secret: &str,
) -> Result<(), AppError> {
    let keys = Keys::parse(raw_secret)
        .map_err(|e| AppError::custom(format!("Invalid key for secure storage: {e}")))?;
    let derived_public_key = keys.public_key().to_hex();
    if derived_public_key != public_key {
        return Err(AppError::custom(format!(
            "Secure storage key mismatch for pubkey {public_key}."
        )));
    }

    let normalized_nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|e| AppError::custom(e.to_string()))?;

    entry(public_key)?
        .set_password(&normalized_nsec)
        .map_err(|e| plugin_error(e.to_string()))?;

    cache_account_keys(app, public_key, &keys)?;
    Ok(())
}

pub fn remove_account_nsec(app: &AppContext, public_key: &str) {
    if let Ok(mut guard) = cache_state(app).keys_by_public_key.lock() {
        guard.remove(public_key);
    }
    if let Ok(entry) = entry(public_key) {
        let _ = entry.delete_credential();
    }
}

pub fn load_account_nsec(_app: &AppContext, public_key: &str) -> Result<String, AppError> {
    entry(public_key)?
        .get_password()
        .map_err(|e| plugin_error(e.to_string()))
}

pub fn keys_for_account(app: &AppContext, public_key: &str) -> Result<Keys, AppError> {
    if let Some(keys) = cached_keys_for_account(app, public_key)? {
        return Ok(keys);
    }

    let nsec = load_account_nsec(app, public_key)?;
    let keys =
        Keys::parse(&nsec).map_err(|e| AppError::custom(format!("Invalid secret key: {e}")))?;

    if keys.public_key().to_hex() != public_key {
        return Err(AppError::custom(format!(
            "Secure storage secret does not match account {public_key}."
        )));
    }

    cache_account_keys(app, public_key, &keys)?;
    Ok(keys)
}

pub fn keys_for_current_identity(
    app: &AppContext,
    conn: &Connection,
) -> Result<(Keys, String), AppError> {
    let public_key = current_identity_public_key(conn)?;

    if let Some(keys) = cached_keys_for_account(app, &public_key)? {
        return Ok((keys, public_key));
    }

    if let Some(nsec) = current_identity_db_nsec(conn)? {
        let keys =
            Keys::parse(&nsec).map_err(|e| AppError::custom(format!("Invalid secret key: {e}")))?;
        if keys.public_key().to_hex() != public_key {
            return Err(AppError::custom(format!(
                "Stored secret does not match account {public_key}."
            )));
        }
        cache_account_keys(app, &public_key, &keys)?;
        return Ok((keys, public_key));
    }

    let keys = keys_for_account(app, &public_key)?;
    Ok((keys, public_key))
}
