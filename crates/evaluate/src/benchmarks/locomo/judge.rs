use anyhow::{Context, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::common::{
    answerer::LlmAnswerer,
    judge::{AnswerJudge, BinaryJudgement, OpenAiCompatibleJudgeRuntime, RetrievalJudge},
    model::{BenchmarkQuestion, GeneratedAnswer},
    prompt::PromptContext,
};

const RETRIEVAL_JUDGE_KIND: &str = "locomo_kioku_retrieval_llm";
const ANSWER_JUDGE_KIND: &str = "locomo_kioku_answer_llm";

const RETRIEVAL_SYSTEM_PROMPT: &str = concat!(
    "You are an evaluator of retrieval quality for a conversational memory benchmark.\n",
    "Judge whether the provided memory prompt alone is sufficient to answer the question with a gold-equivalent answer.\n",
    "The memory prompt may use any textual format chosen by the memory system.\n",
    "Do not judge writing quality. Do not use external knowledge beyond basic language understanding.\n",
    "Return JSON only."
);

const ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You are an evaluator of answer correctness for a conversational memory benchmark.\n",
    "Judge whether the generated answer is semantically equivalent to any gold answer.\n",
    "Be tolerant to wording differences, but strict about wrong entities, wrong dates, and incomplete answers.\n",
    "Return JSON only."
);

#[derive(Debug, Clone)]
pub(crate) struct LoCoMoKiokuRetrievalJudge<T> {
    runtime: OpenAiCompatibleJudgeRuntime<T>,
    prompt_id: String,
}

impl<T> LoCoMoKiokuRetrievalJudge<T> {
    pub(crate) fn new(
        runtime: OpenAiCompatibleJudgeRuntime<T>,
        prompt_id: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            prompt_id: prompt_id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoCoMoKiokuAnswerJudge<T> {
    runtime: OpenAiCompatibleJudgeRuntime<T>,
    prompt_id: String,
}

impl<T> LoCoMoKiokuAnswerJudge<T> {
    pub(crate) fn new(
        runtime: OpenAiCompatibleJudgeRuntime<T>,
        prompt_id: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            prompt_id: prompt_id.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RetrievalJudgePayload {
    label: String,
    supported_answer: Option<String>,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct AnswerJudgePayload {
    label: String,
    reason: String,
}

#[async_trait]
impl<T> RetrievalJudge for LoCoMoKiokuRetrievalJudge<T>
where
    T: LlmAnswerer + Send + Sync,
{
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<BinaryJudgement> {
        let category = question
            .category
            .map_or_else(|| "null".to_string(), |category| category.to_string());
        let user_prompt = format!(
            "Question:\n{}\n\nGold answers:\n{}\n\nQuestion category:\n{}\n\nMemory prompt:\n{}\n\nLabel the prompt as SUFFICIENT if it alone contains enough information to derive a correct answer equivalent to one of the gold answers.\nOtherwise label it INSUFFICIENT.\n\nReturn JSON with:\n- label: SUFFICIENT or INSUFFICIENT\n- supported_answer: short answer or null\n- reason: one short sentence",
            question.question,
            serde_json::to_string_pretty(&question.gold_answers)?,
            category,
            context.text
        );
        let (payload, response) = self
            .runtime
            .generate_json(
                RETRIEVAL_JUDGE_KIND,
                &self.prompt_id,
                RETRIEVAL_SYSTEM_PROMPT,
                user_prompt,
            )
            .await?;
        let payload: RetrievalJudgePayload =
            serde_json::from_value(payload).context("failed to parse retrieval judge JSON")?;
        ensure!(
            matches!(payload.label.as_str(), "SUFFICIENT" | "INSUFFICIENT"),
            "retrieval judge label must be SUFFICIENT or INSUFFICIENT"
        );
        let passed = payload.label == "SUFFICIENT";
        let judge_model = self.runtime.resolved_model_name(&response);

        Ok(BinaryJudgement {
            passed,
            score: if passed { 1.0 } else { 0.0 },
            label: payload.label,
            metadata: json!({
                "judge_kind": RETRIEVAL_JUDGE_KIND,
                "judge_model": judge_model,
                "judge_prompt_id": self.prompt_id,
                "supported_answer": payload.supported_answer,
                "reason": payload.reason,
            }),
        })
    }
}

#[async_trait]
impl<T> AnswerJudge for LoCoMoKiokuAnswerJudge<T>
where
    T: LlmAnswerer + Send + Sync,
{
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<BinaryJudgement> {
        let category = question
            .category
            .map_or_else(|| "null".to_string(), |category| category.to_string());
        let user_prompt = format!(
            "Question:\n{}\n\nGold answers:\n{}\n\nQuestion category:\n{}\n\nGenerated answer:\n{}\n\nReturn JSON with:\n- label: CORRECT or WRONG\n- reason: one short sentence",
            question.question,
            serde_json::to_string_pretty(&question.gold_answers)?,
            category,
            generated.text
        );
        let (payload, response) = self
            .runtime
            .generate_json(
                ANSWER_JUDGE_KIND,
                &self.prompt_id,
                ANSWER_SYSTEM_PROMPT,
                user_prompt,
            )
            .await?;
        let payload: AnswerJudgePayload =
            serde_json::from_value(payload).context("failed to parse answer judge JSON")?;
        ensure!(
            matches!(payload.label.as_str(), "CORRECT" | "WRONG"),
            "answer judge label must be CORRECT or WRONG"
        );
        let passed = payload.label == "CORRECT";
        let judge_model = self.runtime.resolved_model_name(&response);

        Ok(BinaryJudgement {
            passed,
            score: if passed { 1.0 } else { 0.0 },
            label: payload.label,
            metadata: json!({
                "judge_kind": ANSWER_JUDGE_KIND,
                "judge_model": judge_model,
                "judge_prompt_id": self.prompt_id,
                "reason": payload.reason,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use crate::common::{
        answerer::{LlmAnswerer, LlmGenerateRequest, LlmGenerateResponse},
        judge::{AnswerJudge, RetrievalJudge},
        model::{BenchmarkQuestion, GeneratedAnswer, GoldAnswerVariant},
        prompt::{PromptContext, PromptContextKind},
    };

    use super::{LoCoMoKiokuAnswerJudge, LoCoMoKiokuRetrievalJudge, OpenAiCompatibleJudgeRuntime};

    #[derive(Debug, Clone, Default)]
    struct FakeLlm {
        responses: Arc<Mutex<VecDeque<Result<LlmGenerateResponse>>>>,
    }

    impl FakeLlm {
        fn with_responses(responses: Vec<Result<LlmGenerateResponse>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }
    }

    #[async_trait]
    impl LlmAnswerer for FakeLlm {
        async fn generate(
            &self,
            _request: LlmGenerateRequest<'_>,
        ) -> anyhow::Result<LlmGenerateResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(anyhow!("missing fake LLM response")))
        }
    }

    fn sample_question() -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: "q1".to_string(),
            question: "When did it happen?".to_string(),
            question_timestamp: None,
            gold_answers: vec!["May 2019".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: Some(2),
            question_type: None,
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn retrieval_judge_parses_sufficient_json() {
        let judge = LoCoMoKiokuRetrievalJudge::new(
            OpenAiCompatibleJudgeRuntime::new(
                FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
                    text: serde_json::json!({
                        "label": "SUFFICIENT",
                        "supported_answer": "May 2019",
                        "reason": "The context contains the needed time anchor."
                    })
                    .to_string(),
                    model_name: Some("judge-model".to_string()),
                    response_id: None,
                    finish_reason: None,
                    usage: None,
                    raw_response: None,
                })]),
                "judge-model",
                Some(0.0),
                Some(512),
            ),
            "locomo.kioku.judge.retrieval.v1",
        );

        let judgement = judge
            .judge_retrieval(
                &sample_question(),
                &PromptContext {
                    kind: PromptContextKind::MemoryPrompt,
                    text: "1. [fact] It happened in May 2019.".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        assert!(judgement.passed);
        assert_eq!(judgement.label, "SUFFICIENT");
        assert_eq!(judgement.metadata["supported_answer"], "May 2019");
    }

    #[tokio::test]
    async fn answer_judge_rejects_non_json_output() {
        let judge = LoCoMoKiokuAnswerJudge::new(
            OpenAiCompatibleJudgeRuntime::new(
                FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
                    text: "not json".to_string(),
                    model_name: Some("judge-model".to_string()),
                    response_id: None,
                    finish_reason: None,
                    usage: None,
                    raw_response: None,
                })]),
                "judge-model",
                Some(0.0),
                Some(512),
            ),
            "locomo.kioku.judge.answer.v1",
        );

        let error = judge
            .judge_answer(
                &sample_question(),
                &GeneratedAnswer {
                    text: "May 2019".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("expected JSON-only output"));
    }
}
