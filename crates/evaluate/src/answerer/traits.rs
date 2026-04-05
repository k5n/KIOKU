use async_trait::async_trait;

use crate::model::{AnswerRequest, GeneratedAnswer};

#[async_trait]
pub trait Answerer {
    async fn answer(&self, request: AnswerRequest<'_>) -> anyhow::Result<GeneratedAnswer>;
}
