use anyhow::{Context, bail, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::answerer::LlmAnswerer;
use crate::judge::{AnswerJudge, BinaryJudgement, OpenAiCompatibleJudgeRuntime, RetrievalJudge};
use crate::model::{BenchmarkQuestion, GeneratedAnswer};
use crate::prompt::PromptContext;

const RETRIEVAL_JUDGE_KIND: &str = "longmemeval_kioku_retrieval_llm";
const ANSWER_JUDGE_KIND: &str = "longmemeval_kioku_answer_llm";

const RETRIEVAL_SYSTEM_PROMPT: &str = concat!(
    "You evaluate retrieval sufficiency for the LongMemEval Kioku protocol.\n",
    "Judge whether the provided memory prompt alone is sufficient to answer the question with a gold-equivalent answer.\n",
    "The memory prompt may use any textual format chosen by the memory system.\n",
    "Use the provided question type rubric and abstention instructions.\n",
    "Return JSON only."
);

const ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You evaluate answer correctness for the LongMemEval Kioku protocol.\n",
    "Judge whether the generated answer is correct under the provided question type rubric.\n",
    "The memory system may have used any textual memory-prompt format.\n",
    "Use the abstention instructions when the question is marked as abstention.\n",
    "Return JSON only."
);

#[derive(Debug, Clone)]
pub struct LongMemEvalKiokuRetrievalJudge<T> {
    runtime: OpenAiCompatibleJudgeRuntime<T>,
    prompt_id: String,
}

impl<T> LongMemEvalKiokuRetrievalJudge<T> {
    pub fn new(runtime: OpenAiCompatibleJudgeRuntime<T>, prompt_id: impl Into<String>) -> Self {
        Self {
            runtime,
            prompt_id: prompt_id.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LongMemEvalKiokuAnswerJudge<T> {
    runtime: OpenAiCompatibleJudgeRuntime<T>,
    prompt_id: String,
}

impl<T> LongMemEvalKiokuAnswerJudge<T> {
    pub fn new(runtime: OpenAiCompatibleJudgeRuntime<T>, prompt_id: impl Into<String>) -> Self {
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
impl<T> RetrievalJudge for LongMemEvalKiokuRetrievalJudge<T>
where
    T: LlmAnswerer + Send + Sync,
{
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<BinaryJudgement> {
        let user_prompt = render_retrieval_user_prompt(question, context)?;
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
impl<T> AnswerJudge for LongMemEvalKiokuAnswerJudge<T>
where
    T: LlmAnswerer + Send + Sync,
{
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<BinaryJudgement> {
        let user_prompt = render_answer_user_prompt(question, generated)?;
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

fn render_retrieval_user_prompt(
    question: &BenchmarkQuestion,
    context: &PromptContext,
) -> anyhow::Result<String> {
    let question_type = longmemeval_question_type(question)?;
    let question_date = longmemeval_question_date(question)?;
    let rubric = longmemeval_type_rubric(question_type)?;
    Ok(format!(
        concat!(
            "Question:\n{}\n\n",
            "Gold answers:\n{}\n\n",
            "Question type:\n{}\n\n",
            "Question date:\n{}\n\n",
            "Is abstention:\n{}\n\n",
            "Type-specific rubric:\n{}\n\n",
            "Abstention instructions:\n{}\n\n",
            "Memory prompt:\n{}\n\n",
            "Return JSON with:\n",
            "- label: SUFFICIENT or INSUFFICIENT\n",
            "- supported_answer: short answer or null\n",
            "- reason: one short sentence"
        ),
        question.question,
        serde_json::to_string_pretty(&question.gold_answers)?,
        question_type,
        question_date,
        question.is_abstention,
        rubric,
        abstention_guidance(question.is_abstention),
        context.text
    ))
}

fn render_answer_user_prompt(
    question: &BenchmarkQuestion,
    generated: &GeneratedAnswer,
) -> anyhow::Result<String> {
    let question_type = longmemeval_question_type(question)?;
    let question_date = longmemeval_question_date(question)?;
    let rubric = longmemeval_type_rubric(question_type)?;
    Ok(format!(
        concat!(
            "Question:\n{}\n\n",
            "Gold answers:\n{}\n\n",
            "Question type:\n{}\n\n",
            "Question date:\n{}\n\n",
            "Is abstention:\n{}\n\n",
            "Type-specific rubric:\n{}\n\n",
            "Abstention instructions:\n{}\n\n",
            "Generated answer:\n{}\n\n",
            "Return JSON with:\n",
            "- label: CORRECT or WRONG\n",
            "- reason: one short sentence"
        ),
        question.question,
        serde_json::to_string_pretty(&question.gold_answers)?,
        question_type,
        question_date,
        question.is_abstention,
        rubric,
        abstention_guidance(question.is_abstention),
        generated.text
    ))
}

fn longmemeval_question_type(question: &BenchmarkQuestion) -> anyhow::Result<&str> {
    question
        .question_type
        .as_deref()
        .context("LongMemEval Kioku judge requires question_type")
}

fn longmemeval_question_date(question: &BenchmarkQuestion) -> anyhow::Result<&str> {
    question
        .metadata
        .get("raw_question_date")
        .and_then(serde_json::Value::as_str)
        .or(question.question_timestamp.as_deref())
        .context("LongMemEval Kioku judge requires question_date")
}

fn longmemeval_type_rubric(question_type: &str) -> anyhow::Result<&'static str> {
    match question_type {
        "single-session-user" => Ok(
            "Judge whether the answer correctly identifies a user-provided detail from one session.",
        ),
        "single-session-assistant" => Ok(
            "Judge whether the answer correctly identifies an assistant-provided detail from one session.",
        ),
        "single-session-preference" => Ok(
            "Judge whether the answer correctly identifies the user's preference stated in one session.",
        ),
        "temporal-reasoning" => Ok(
            "Judge whether the answer correctly resolves the requested time or ordering relation.",
        ),
        "knowledge-update" => Ok(
            "Judge whether the answer reflects the latest state supported by memory as of the question date.",
        ),
        "multi-session" => {
            Ok("Judge whether the answer correctly combines evidence across multiple sessions.")
        }
        other => bail!("unsupported LongMemEval question_type `{other}`"),
    }
}

fn abstention_guidance(is_abstention: bool) -> &'static str {
    if is_abstention {
        "A correct result should abstain. Accept NOT_ENOUGH_MEMORY or a semantically equivalent abstention."
    } else {
        "A correct result should provide an answer, not abstain."
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use crate::answerer::{LlmAnswerer, LlmGenerateRequest, LlmGenerateResponse};
    use crate::judge::{AnswerJudge, RetrievalJudge};
    use crate::model::{BenchmarkQuestion, GeneratedAnswer, GoldAnswerVariant};
    use crate::prompt::{PromptContext, PromptContextKind};

    use super::{
        ANSWER_SYSTEM_PROMPT, LongMemEvalKiokuAnswerJudge, LongMemEvalKiokuRetrievalJudge,
        OpenAiCompatibleJudgeRuntime, RETRIEVAL_SYSTEM_PROMPT,
    };

    #[derive(Debug, Clone, Default)]
    struct FakeLlm {
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
        responses: Arc<Mutex<VecDeque<Result<LlmGenerateResponse>>>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedRequest {
        system_prompt: String,
        user_prompt: String,
    }

    impl FakeLlm {
        fn with_responses(responses: Vec<Result<LlmGenerateResponse>>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }

        fn captured_requests(&self) -> Vec<CapturedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmAnswerer for FakeLlm {
        async fn generate(
            &self,
            request: LlmGenerateRequest<'_>,
        ) -> anyhow::Result<LlmGenerateResponse> {
            self.requests.lock().unwrap().push(CapturedRequest {
                system_prompt: request.system_prompt.unwrap_or_default().to_string(),
                user_prompt: request.user_prompt.to_string(),
            });
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
            question: "Where does the user live now?".to_string(),
            question_timestamp: Some("2024-01-03T00:00:00Z".to_string()),
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: vec!["s1".to_string()],
            category: None,
            question_type: Some("knowledge-update".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::json!({
                "raw_question_date": "2024-01-03",
            }),
        }
    }

    #[tokio::test]
    async fn retrieval_judge_parses_sufficient_json() {
        let judge = LongMemEvalKiokuRetrievalJudge::new(
            OpenAiCompatibleJudgeRuntime::new(
                FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
                    text: serde_json::json!({
                        "label": "SUFFICIENT",
                        "supported_answer": "Kyoto",
                        "reason": "The latest location is explicitly stated."
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
            "longmemeval.kioku.judge.retrieval.v1",
        );

        let judgement = judge
            .judge_retrieval(
                &sample_question(),
                &PromptContext {
                    kind: PromptContextKind::MemoryPrompt,
                    text: "user: I moved to Kyoto last month.".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        assert!(judgement.passed);
        assert_eq!(judgement.label, "SUFFICIENT");
        assert_eq!(
            judgement.metadata["judge_kind"],
            "longmemeval_kioku_retrieval_llm"
        );
        assert_eq!(judgement.metadata["supported_answer"], "Kyoto");
    }

    #[tokio::test]
    async fn answer_judge_parses_correct_json() {
        let judge = LongMemEvalKiokuAnswerJudge::new(
            OpenAiCompatibleJudgeRuntime::new(
                FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
                    text: serde_json::json!({
                        "label": "CORRECT",
                        "reason": "The generated answer matches the latest state."
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
            "longmemeval.kioku.judge.answer.v1",
        );

        let judgement = judge
            .judge_answer(
                &sample_question(),
                &GeneratedAnswer {
                    text: "Kyoto".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        assert!(judgement.passed);
        assert_eq!(judgement.label, "CORRECT");
        assert_eq!(
            judgement.metadata["judge_kind"],
            "longmemeval_kioku_answer_llm"
        );
    }

    #[tokio::test]
    async fn retrieval_prompt_contains_required_longmemeval_fields() {
        let llm = FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
            text: serde_json::json!({
                "label": "INSUFFICIENT",
                "supported_answer": null,
                "reason": "stub"
            })
            .to_string(),
            model_name: None,
            response_id: None,
            finish_reason: None,
            usage: None,
            raw_response: None,
        })]);
        let judge = LongMemEvalKiokuRetrievalJudge::new(
            OpenAiCompatibleJudgeRuntime::new(llm.clone(), "judge-model", Some(0.0), Some(512)),
            "longmemeval.kioku.judge.retrieval.v1",
        );
        let mut question = sample_question();
        question.is_abstention = true;

        judge
            .judge_retrieval(
                &question,
                &PromptContext {
                    kind: PromptContextKind::MemoryPrompt,
                    text: "context text".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        let request = llm.captured_requests().pop().unwrap();
        assert_eq!(request.system_prompt, RETRIEVAL_SYSTEM_PROMPT);
        assert!(
            request
                .user_prompt
                .contains("Question type:\nknowledge-update")
        );
        assert!(request.user_prompt.contains("Question date:\n2024-01-03"));
        assert!(request.user_prompt.contains("Is abstention:\ntrue"));
        assert!(request.user_prompt.contains("Memory prompt:\ncontext text"));
        assert!(
            request
                .user_prompt
                .contains("latest state supported by memory")
        );
    }

    #[tokio::test]
    async fn answer_prompt_contains_required_longmemeval_fields() {
        let llm = FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
            text: serde_json::json!({
                "label": "WRONG",
                "reason": "stub"
            })
            .to_string(),
            model_name: None,
            response_id: None,
            finish_reason: None,
            usage: None,
            raw_response: None,
        })]);
        let judge = LongMemEvalKiokuAnswerJudge::new(
            OpenAiCompatibleJudgeRuntime::new(llm.clone(), "judge-model", Some(0.0), Some(512)),
            "longmemeval.kioku.judge.answer.v1",
        );

        judge
            .judge_answer(
                &sample_question(),
                &GeneratedAnswer {
                    text: "Osaka".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        let request = llm.captured_requests().pop().unwrap();
        assert_eq!(request.system_prompt, ANSWER_SYSTEM_PROMPT);
        assert!(
            request
                .user_prompt
                .contains("Question type:\nknowledge-update")
        );
        assert!(request.user_prompt.contains("Question date:\n2024-01-03"));
        assert!(request.user_prompt.contains("Is abstention:\nfalse"));
        assert!(request.user_prompt.contains("Generated answer:\nOsaka"));
    }

    #[tokio::test]
    async fn retrieval_judge_rejects_unknown_question_type() {
        let llm = FakeLlm::with_responses(Vec::new());
        let judge = LongMemEvalKiokuRetrievalJudge::new(
            OpenAiCompatibleJudgeRuntime::new(llm, "judge-model", Some(0.0), Some(512)),
            "longmemeval.kioku.judge.retrieval.v1",
        );
        let mut question = sample_question();
        question.question_type = Some("unknown-type".to_string());

        let error = judge
            .judge_retrieval(
                &question,
                &PromptContext {
                    kind: PromptContextKind::MemoryPrompt,
                    text: "context text".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unsupported LongMemEval question_type `unknown-type`"));
    }
}
