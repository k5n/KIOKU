pub trait TokenCounter: Send + Sync {
    fn count_text_tokens(&self, text: &str) -> anyhow::Result<usize>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WhitespaceTokenCounter;

impl WhitespaceTokenCounter {
    pub const NAME: &'static str = "whitespace_v1";
}

impl TokenCounter for WhitespaceTokenCounter {
    fn count_text_tokens(&self, text: &str) -> anyhow::Result<usize> {
        Ok(text.split_whitespace().count())
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }
}

#[cfg(test)]
mod tests {
    use super::{TokenCounter, WhitespaceTokenCounter};

    #[test]
    fn whitespace_counter_counts_split_whitespace_tokens() {
        let counter = WhitespaceTokenCounter;
        assert_eq!(counter.count_text_tokens("alpha beta\ngamma").unwrap(), 3);
        assert_eq!(counter.name(), "whitespace_v1");
    }
}
