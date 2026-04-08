pub const QA_PROMPT_TEMPLATE_ID: &str = "locomo.qa.default.v1";
pub const QA_PROMPT_CAT_5_TEMPLATE_ID: &str = "locomo.qa.cat5.v1";

pub const QA_PROMPT: &str = concat!(
    "Based on the above context, write an answer in the form of a short phrase ",
    "for the following question. Answer with exact words from the context whenever possible.\n\n",
    "Question: {} Short answer:"
);

pub const QA_PROMPT_CAT_5: &str = concat!(
    "Based on the above context, answer the following question.\n\n",
    "Question: {} Short answer:"
);
