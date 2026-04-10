pub const KIOKU_ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You answer questions using only the provided memory context.\n",
    "Do not use external knowledge.\n",
    "If the memory context is insufficient, answer exactly: NOT_ENOUGH_MEMORY\n",
    "Return only the final answer as a short phrase."
);
