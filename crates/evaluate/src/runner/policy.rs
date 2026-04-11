#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTokenPolicy {
    Optional,
    Required,
}

impl ContextTokenPolicy {
    pub fn requires_count(self) -> bool {
        matches!(self, Self::Required)
    }
}
