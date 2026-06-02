use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};

const APPLICATION_USER_KEY: &str = "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorBaseUrlUpdate {
    pub db_path: PathBuf,
    pub old_url: Option<String>,
    pub new_url: String,
    pub backup_path: PathBuf,
}

pub fn default_cursor_state_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
}

pub fn update_cursor_openai_base_url(
    db_path: &Path,
    new_url: &str,
) -> AppResult<CursorBaseUrlUpdate> {
    if !db_path.exists() {
        return Err(AppError::Config(format!(
            "Cursor state database not found: {}",
            db_path.display()
        )));
    }

    let backup_path = db_path.with_extension("vscdb.bak");
    fs::copy(db_path, &backup_path).map_err(|error| {
        AppError::Config(format!(
            "failed to back up Cursor state database {} -> {}: {error}",
            db_path.display(),
            backup_path.display()
        ))
    })?;

    let mut connection = Connection::open(db_path).map_err(|error| {
        AppError::Config(format!("failed to open Cursor state database: {error}"))
    })?;
    let transaction = connection.transaction().map_err(|error| {
        AppError::Config(format!("failed to start Cursor state transaction: {error}"))
    })?;

    let current_value: String = transaction
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [APPLICATION_USER_KEY],
            |row| row.get(0),
        )
        .map_err(|error| {
            AppError::Config(format!(
                "failed to load Cursor application user state from {}: {error}",
                db_path.display()
            ))
        })?;

    let mut payload: Value = serde_json::from_str(&current_value).map_err(|error| {
        AppError::Config(format!(
            "failed to parse Cursor application user JSON from {}: {error}",
            db_path.display()
        ))
    })?;

    let old_url = payload
        .get("openAIBaseUrl")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    if !payload.is_object() {
        payload = json!({});
    }

    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "openAIBaseUrl".to_string(),
            Value::String(new_url.to_string()),
        );
    }

    let encoded = serde_json::to_string(&payload).map_err(|error| {
        AppError::Config(format!(
            "failed to encode updated Cursor application user JSON for {}: {error}",
            db_path.display()
        ))
    })?;

    transaction
        .execute(
            "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
            params![encoded, APPLICATION_USER_KEY],
        )
        .map_err(|error| {
            AppError::Config(format!(
                "failed to update Cursor application user state in {}: {error}",
                db_path.display()
            ))
        })?;

    transaction.commit().map_err(|error| {
        AppError::Config(format!(
            "failed to commit Cursor state database update for {}: {error}",
            db_path.display()
        ))
    })?;

    Ok(CursorBaseUrlUpdate {
        db_path: db_path.to_path_buf(),
        old_url,
        new_url: new_url.to_string(),
        backup_path,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        APPLICATION_USER_KEY, default_cursor_state_db_path, update_cursor_openai_base_url,
    };
    use rusqlite::Connection;
    use serde_json::Value;

    #[test]
    fn default_cursor_state_db_path_points_to_global_storage() {
        let path = default_cursor_state_db_path();
        let rendered = path.to_string_lossy();
        assert!(rendered.contains("Cursor/User/globalStorage/state.vscdb"));
    }

    #[test]
    fn updates_existing_openai_base_url_and_creates_backup() {
        let dir =
            std::env::temp_dir().join(format!("dcp-cursor-state-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("state.vscdb");
        let backup_path = dir.join("state.vscdb.bak");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&backup_path);

        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute(
                "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                (
                    APPLICATION_USER_KEY,
                    r#"{"openAIBaseUrl":"https://old.example.com","useOpenAIKey":true}"#,
                ),
            )
            .unwrap();
        drop(connection);

        let result = update_cursor_openai_base_url(&db_path, "https://new.example.com").unwrap();
        assert_eq!(result.old_url.as_deref(), Some("https://old.example.com"));
        assert_eq!(result.new_url, "https://new.example.com");
        assert!(backup_path.exists());

        let connection = Connection::open(&db_path).unwrap();
        let encoded: String = connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                [APPLICATION_USER_KEY],
                |row| row.get(0),
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            payload.get("openAIBaseUrl").and_then(Value::as_str),
            Some("https://new.example.com")
        );
    }
}
