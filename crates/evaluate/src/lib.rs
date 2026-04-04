pub fn dataset_banner(dataset: &str) -> String {
    format!("evaluate runner for {dataset}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dataset_banner() {
        let result = dataset_banner("LoCoMo");
        assert_eq!(result, "evaluate runner for LoCoMo");
    }
}
