use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::trace::model::TraceSummary;

#[derive(Debug)]
pub struct TraceWriter {
    session_dir: PathBuf,
    sequence: Mutex<u64>,
}

impl TraceWriter {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Result<Self, AppError> {
        let session_name = format!(
            "{}-pid{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("after epoch")
                .as_millis(),
            std::process::id()
        );
        let session_dir = base_dir.as_ref().join(session_name);
        std::fs::create_dir_all(&session_dir)
            .map_err(|error| AppError::Trace(error.to_string()))?;
        Ok(Self {
            session_dir,
            sequence: Mutex::new(1),
        })
    }

    pub fn write(&self, mut summary: TraceSummary) -> Result<(), AppError> {
        let mut sequence = self
            .sequence
            .lock()
            .map_err(|_| AppError::Trace("trace writer lock poisoned".to_string()))?;
        summary.sequence = *sequence;
        *sequence += 1;

        let path = self
            .session_dir
            .join(format!("request-{:06}.json", summary.sequence));
        let json = serde_json::to_vec_pretty(&summary)
            .map_err(|error| AppError::Trace(error.to_string()))?;
        std::fs::write(path, json).map_err(|error| AppError::Trace(error.to_string()))
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::TraceWriter;
    use crate::trace::model::TraceSummary;

    #[test]
    fn writes_trace_file() {
        let base_dir = std::env::temp_dir().join("deepseek-cursor-proxy-rust-trace-tests");
        let writer = TraceWriter::new(&base_dir).unwrap();
        writer
            .write(TraceSummary {
                sequence: 0,
                method: "POST".to_string(),
                path: "/v1/chat/completions".to_string(),
                request_body: Some(json!({"ok": true})),
                response_status: Some(200),
            })
            .unwrap();
        let files = std::fs::read_dir(writer.session_dir()).unwrap().count();
        assert_eq!(files, 1);
    }
}
