use crate::prompt::LongMemEvalAnswerPromptProfile;

pub fn template_id(profile: LongMemEvalAnswerPromptProfile, cot: bool) -> &'static str {
    match (profile, cot) {
        (LongMemEvalAnswerPromptProfile::NoRetrieval, false) => {
            "longmemeval.answer.no_retrieval.v1"
        }
        (LongMemEvalAnswerPromptProfile::NoRetrieval, true) => {
            "longmemeval.answer.no_retrieval.cot.v1"
        }
        (LongMemEvalAnswerPromptProfile::HistoryChats, false) => {
            "longmemeval.answer.history_chats.v1"
        }
        (LongMemEvalAnswerPromptProfile::HistoryChats, true) => {
            "longmemeval.answer.history_chats.cot.v1"
        }
        (LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts, false) => {
            "longmemeval.answer.history_chats_with_facts.v1"
        }
        (LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts, true) => {
            "longmemeval.answer.history_chats_with_facts.cot.v1"
        }
        (LongMemEvalAnswerPromptProfile::FactsOnly, false) => "longmemeval.answer.facts_only.v1",
        (LongMemEvalAnswerPromptProfile::FactsOnly, true) => "longmemeval.answer.facts_only.cot.v1",
    }
}

pub fn render_prompt(
    profile: LongMemEvalAnswerPromptProfile,
    cot: bool,
    context_text: &str,
    current_date: &str,
    question: &str,
) -> String {
    match (profile, cot) {
        (LongMemEvalAnswerPromptProfile::NoRetrieval, false) => question.to_string(),
        (LongMemEvalAnswerPromptProfile::NoRetrieval, true) => {
            format!("{question}\nAnswer step by step.")
        }
        (LongMemEvalAnswerPromptProfile::HistoryChats, false) => format!(
            concat!(
                "I will give you several history chats between you and a user. ",
                "Please answer the question based on the relevant chat history.\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer:"
            ),
            context_text, current_date, question
        ),
        (LongMemEvalAnswerPromptProfile::HistoryChats, true) => format!(
            concat!(
                "I will give you several history chats between you and a user. ",
                "Please answer the question based on the relevant chat history. ",
                "Answer the question step by step: first extract all the relevant information, ",
                "and then reason over the information to get the answer.\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer (step by step):"
            ),
            context_text, current_date, question
        ),
        (LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts, false) => format!(
            concat!(
                "I will give you several history chats between you and a user, as well as the ",
                "relevant user facts extracted from the chat history. Please answer the question ",
                "based on the relevant chat history and the user facts\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer:"
            ),
            context_text, current_date, question
        ),
        (LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts, true) => format!(
            concat!(
                "I will give you several history chats between you and a user, as well as the ",
                "relevant user facts extracted from the chat history. Please answer the question ",
                "based on the relevant chat history and the user facts. Answer the question ",
                "step by step: first extract all the relevant information, and then reason over ",
                "the information to get the answer.\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer (step by step):"
            ),
            context_text, current_date, question
        ),
        (LongMemEvalAnswerPromptProfile::FactsOnly, false) => format!(
            concat!(
                "I will give you several facts extracted from history chats between you and a user. ",
                "Please answer the question based on the relevant facts.\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer:"
            ),
            context_text, current_date, question
        ),
        (LongMemEvalAnswerPromptProfile::FactsOnly, true) => format!(
            concat!(
                "I will give you several facts extracted from history chats between you and a user. ",
                "Please answer the question based on the relevant facts. Answer the question step ",
                "by step: first extract all the relevant information, and then reason over the ",
                "information to get the answer.\n\n\n",
                "History Chats:\n\n{}\n\n",
                "Current Date: {}\n",
                "Question: {}\n",
                "Answer (step by step):"
            ),
            context_text, current_date, question
        ),
    }
}
