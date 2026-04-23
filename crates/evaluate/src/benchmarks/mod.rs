mod locomo;
mod longmemeval;

use crate::common::{
    answerer::Answerer,
    backend::MemoryBackend,
    judge::{AnswerJudge, RetrievalJudge},
    model::{BenchmarkCase, RetrievalBudget},
    prompt::PromptBuilder,
    runner::{DatasetEvaluationProtocol, EvaluatePipelineResult, run_pipeline},
    token_counter::TokenCounter,
};

pub(crate) use locomo::{
    BenchmarkConfig as LoCoMoBenchmarkConfig, LocomoKiokuPromptConfig,
    TomlBenchmarkSection as TomlLoCoMoBenchmarkSection,
};
pub(crate) use longmemeval::{
    BenchmarkConfig as LongMemEvalBenchmarkConfig, LongMemEvalKiokuPromptConfig,
    TomlBenchmarkSection as TomlLongMemEvalBenchmarkSection,
};

pub(crate) struct PreparedBenchmarkRun<PB, P, AJ, RJ> {
    pub(crate) cases: Vec<BenchmarkCase>,
    pub(crate) prompt_builder: PB,
    pub(crate) protocol: P,
    pub(crate) answer_judge: AJ,
    pub(crate) retrieval_judge: RJ,
    pub(crate) token_counter: Option<Box<dyn TokenCounter>>,
}

pub(crate) async fn execute_prepared_run<B, A, PB, P, AJ, RJ>(
    prepared: PreparedBenchmarkRun<PB, P, AJ, RJ>,
    backend: &mut B,
    answerer: &A,
    budget: RetrievalBudget,
) -> anyhow::Result<EvaluatePipelineResult>
where
    B: MemoryBackend + ?Sized,
    A: Answerer + ?Sized,
    PB: PromptBuilder,
    P: DatasetEvaluationProtocol,
    AJ: AnswerJudge,
    RJ: RetrievalJudge,
{
    let token_counter = prepared.token_counter.as_deref();
    run_pipeline(
        &prepared.cases,
        backend,
        &prepared.prompt_builder,
        answerer,
        &prepared.answer_judge,
        &prepared.retrieval_judge,
        token_counter,
        budget,
        &prepared.protocol,
    )
    .await
}

pub(crate) mod locomo_api {
    pub(crate) use super::locomo::{
        prepare_run, prompt_config_metadata, resolve_config, validate_config,
    };
}

pub(crate) mod longmemeval_api {
    pub(crate) use super::longmemeval::{
        prepare_run, prompt_config_metadata, resolve_config, validate_config,
    };
}
