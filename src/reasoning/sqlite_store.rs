use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};

use crate::error::AppError;
use crate::protocol::model::Message;

use super::keys::{portable_reasoning_keys, scoped_reasoning_keys};
use super::store::ReasoningStore;

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs_f64()
}

#[derive(Debug)]
pub struct SqliteReasoningStore {
    conn: Mutex<Connection>,
    _path: PathBuf,
    max_age_seconds: Option<u64>,
    max_rows: Option<usize>,
}

impl SqliteReasoningStore {
    pub fn new<P: AsRef<Path>>(
        path: P,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
    ) -> Result<Self, AppError> {
        let path_ref = path.as_ref();
        if path_ref != Path::new(":memory:") {
            if let Some(parent) = path_ref.parent() {
                fs::create_dir_all(parent).map_err(|error| AppError::Storage(error.to_string()))?;
            }
        }

        let conn =
            Connection::open(path_ref).map_err(|error| AppError::Storage(error.to_string()))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS reasoning_cache (
                key TEXT PRIMARY KEY,
                reasoning TEXT NOT NULL,
                message_json TEXT NOT NULL,
                created_at REAL NOT NULL
            )",
            [],
        )
        .map_err(|error| AppError::Storage(error.to_string()))?;

        if path_ref != Path::new(":memory:") {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = fs::Permissions::from_mode(0o600);
                fs::set_permissions(path_ref, permissions)
                    .map_err(|error| AppError::Storage(error.to_string()))?;
            }
        }

        let store = Self {
            conn: Mutex::new(conn),
            _path: path_ref.to_path_buf(),
            max_age_seconds,
            max_rows,
        };
        store.prune()?;
        Ok(store)
    }

    fn with_conn<T>(
        &self,
        callback: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Storage("reasoning store lock poisoned".to_string()))?;
        callback(&conn)
    }
}

impl ReasoningStore for SqliteReasoningStore {
    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT reasoning FROM reasoning_cache WHERE key = ?1",
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            Ok(row)
        })
    }

    fn put(&self, key: &str, reasoning: &str, message: &Message) -> Result<(), AppError> {
        let message_json =
            serde_json::to_string(message).map_err(|error| AppError::Storage(error.to_string()))?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO reasoning_cache(key, reasoning, message_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(key) DO UPDATE SET
                    reasoning = excluded.reasoning,
                    message_json = excluded.message_json,
                    created_at = excluded.created_at",
                params![key, reasoning, message_json, now_ts()],
            )
            .map_err(|error| AppError::Storage(error.to_string()))?;
            Ok(())
        })?;
        self.prune()?;
        Ok(())
    }

    fn prune(&self) -> Result<usize, AppError> {
        self.with_conn(|conn| {
            let mut deleted = 0usize;
            if let Some(max_age_seconds) = self.max_age_seconds.filter(|value| *value > 0) {
                let cutoff = now_ts() - max_age_seconds as f64;
                let rows = conn
                    .execute(
                        "DELETE FROM reasoning_cache WHERE created_at < ?1",
                        [cutoff],
                    )
                    .map_err(|error| AppError::Storage(error.to_string()))?;
                deleted += rows;
            }

            if let Some(max_rows) = self.max_rows.filter(|value| *value > 0) {
                let rows = conn
                    .execute(
                        "DELETE FROM reasoning_cache
                         WHERE key NOT IN (
                            SELECT key FROM reasoning_cache
                            ORDER BY created_at DESC
                            LIMIT ?1
                         )",
                        [max_rows as i64],
                    )
                    .map_err(|error| AppError::Storage(error.to_string()))?;
                deleted += rows;
            }
            Ok(deleted)
        })
    }

    fn clear(&self) -> Result<usize, AppError> {
        self.with_conn(|conn| {
            let count =
                conn.query_row("SELECT COUNT(*) FROM reasoning_cache", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|error| AppError::Storage(error.to_string()))? as usize;
            conn.execute("DELETE FROM reasoning_cache", [])
                .map_err(|error| AppError::Storage(error.to_string()))?;
            Ok(count)
        })
    }

    fn store_assistant_message(
        &self,
        message: &Message,
        scope: &str,
        cache_namespace: &str,
        prior_messages: Option<&[Message]>,
    ) -> Result<usize, AppError> {
        if message.role != "assistant" {
            return Ok(0);
        }
        let Some(reasoning) = &message.reasoning_content else {
            return Ok(0);
        };

        let mut keys = scoped_reasoning_keys(message, scope);
        if let Some(prior_messages) = prior_messages {
            keys.extend(portable_reasoning_keys(
                message,
                cache_namespace,
                prior_messages,
            ));
        }
        keys.sort();
        keys.dedup();

        for key in &keys {
            self.put(key, reasoning, message)?;
        }
        Ok(keys.len())
    }

    fn lookup_for_message(
        &self,
        message: &Message,
        scope: &str,
        cache_namespace: &str,
        prior_messages: Option<&[Message]>,
    ) -> Result<Option<String>, AppError> {
        let mut keys = scoped_reasoning_keys(message, scope);
        if let Some(prior_messages) = prior_messages {
            keys.extend(portable_reasoning_keys(
                message,
                cache_namespace,
                prior_messages,
            ));
        }
        for key in keys {
            if let Some(reasoning) = self.get(&key)? {
                return Ok(Some(reasoning));
            }
        }
        Ok(None)
    }

    fn backfill_portable_aliases(
        &self,
        message: &Message,
        reasoning: &str,
        cache_namespace: &str,
        prior_messages: &[Message],
    ) -> Result<usize, AppError> {
        let mut keys = portable_reasoning_keys(message, cache_namespace, prior_messages);
        if keys.is_empty() {
            return Ok(0);
        }
        keys.sort();
        keys.dedup();

        let mut message_with_reasoning = message.clone();
        message_with_reasoning.reasoning_content = Some(reasoning.to_string());

        for key in &keys {
            self.put(key, reasoning, &message_with_reasoning)?;
        }
        Ok(keys.len())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::SqliteReasoningStore;
    use crate::protocol::model::{Message, ToolCall, ToolFunction};
    use crate::reasoning::keys::conversation_scope;
    use crate::reasoning::store::ReasoningStore;

    fn temp_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join("deepseek-cursor-proxy-rust-tests")
            .join(format!("reasoning-{unique}.sqlite3"))
    }

    fn tool_call() -> ToolCall {
        ToolCall {
            id: Some("call_empty".to_string()),
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "lookup".to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    #[test]
    fn file_store_creates_database_file() {
        let path = temp_path();
        let store = SqliteReasoningStore::new(&path, None, None).unwrap();
        drop(store);
        assert!(path.exists());
    }

    #[test]
    fn store_prunes_to_max_rows_and_can_clear() {
        let store = SqliteReasoningStore::new(":memory:", None, Some(2)).unwrap();
        let message = Message {
            role: "assistant".to_string(),
            ..Message::default()
        };
        store.put("a", "reasoning a", &message).unwrap();
        store.put("b", "reasoning b", &message).unwrap();
        store.put("c", "reasoning c", &message).unwrap();

        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("b").unwrap(), Some("reasoning b".to_string()));
        assert_eq!(store.get("c").unwrap(), Some("reasoning c".to_string()));
        assert_eq!(store.clear().unwrap(), 2);
        assert_eq!(store.get("b").unwrap(), None);
    }

    #[test]
    fn stores_empty_reasoning_as_present_value() {
        let store = SqliteReasoningStore::new(":memory:", None, None).unwrap();
        let scope = conversation_scope(
            &[Message {
                role: "user".to_string(),
                content: Some(json!("lookup")),
                ..Message::default()
            }],
            "",
        );
        let message = Message {
            role: "assistant".to_string(),
            content: Some(json!("")),
            reasoning_content: Some(String::new()),
            tool_calls: Some(vec![tool_call()]),
            ..Message::default()
        };

        assert!(
            store
                .store_assistant_message(&message, &scope, "", None)
                .unwrap()
                > 0
        );
        assert_eq!(
            store
                .get(&format!("scope:{scope}:tool_call:call_empty"))
                .unwrap(),
            Some(String::new())
        );

        let lookup = Message {
            role: "assistant".to_string(),
            content: Some(json!("")),
            tool_calls: Some(vec![tool_call()]),
            ..Message::default()
        };
        assert_eq!(
            store.lookup_for_message(&lookup, &scope, "", None).unwrap(),
            Some(String::new())
        );
    }
}
