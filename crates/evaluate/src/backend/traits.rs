use async_trait::async_trait;

use crate::model::{BenchmarkEvent, EvalScope, QueryInput, QueryOutput};

#[async_trait]
pub trait MemoryBackend {
    async fn reset(&mut self, scope: EvalScope) -> anyhow::Result<()>;
    async fn ingest(&mut self, event: BenchmarkEvent) -> anyhow::Result<()>;
    async fn query(&mut self, input: QueryInput) -> anyhow::Result<QueryOutput>;
}
