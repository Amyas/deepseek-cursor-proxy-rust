use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceSummary {
    pub sequence: u64,
    pub method: String,
    pub path: String,
    pub request_body: Option<Value>,
    pub response_status: Option<u16>,
}
