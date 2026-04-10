use crate::model::{AnswerLogRecord, MetricsReport, RetrievalLogRecord};

#[derive(Debug)]
pub struct EvaluatePipelineResult {
    pub answers: Vec<AnswerLogRecord>,
    pub retrievals: Vec<RetrievalLogRecord>,
    pub metrics: MetricsReport,
}
