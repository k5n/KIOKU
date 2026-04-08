#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
pub(super) fn write_temp_config(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kioku-evaluate-config-{name}-{}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("run.toml");
    std::fs::write(&path, body).unwrap();
    path
}
