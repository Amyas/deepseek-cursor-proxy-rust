use crate::error::AppError;
use crate::protocol::model::Message;

pub trait ReasoningStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    fn put(&self, key: &str, reasoning: &str, message: &Message) -> Result<(), AppError>;
    fn prune(&self) -> Result<usize, AppError>;
    fn clear(&self) -> Result<usize, AppError>;
    fn store_assistant_message(
        &self,
        message: &Message,
        scope: &str,
        cache_namespace: &str,
        prior_messages: Option<&[Message]>,
    ) -> Result<usize, AppError>;
    fn lookup_for_message(
        &self,
        message: &Message,
        scope: &str,
        cache_namespace: &str,
        prior_messages: Option<&[Message]>,
    ) -> Result<Option<String>, AppError>;
    fn backfill_portable_aliases(
        &self,
        message: &Message,
        reasoning: &str,
        cache_namespace: &str,
        prior_messages: &[Message],
    ) -> Result<usize, AppError>;
}
