use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::reasoning::sqlite_store::SqliteReasoningStore;
use crate::trace::writer::TraceWriter;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub client: reqwest::Client,
    pub store: Arc<SqliteReasoningStore>,
    pub trace_writer: Option<Arc<TraceWriter>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .map_err(|error| AppError::Upstream(error.to_string()))?;
        let store = Arc::new(SqliteReasoningStore::new(
            &config.reasoning_content_path,
            None,
            None,
        )?);
        let trace_writer = config
            .trace_dir
            .as_ref()
            .map(TraceWriter::new)
            .transpose()?
            .map(Arc::new);
        Ok(Self {
            config,
            client,
            store,
            trace_writer,
        })
    }
}
