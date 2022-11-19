use tokio::fs;
use serde::de::DeserializeOwned;
use anyhow::{anyhow, Context, Result};
use tokio::io::AsyncReadExt;

pub async fn load_json_config<T: DeserializeOwned>(file_id: &str) -> Result<T> {
    let project_dirs = directories::ProjectDirs::from("net", "oksenenko", "anki-words-importer")
        .ok_or(anyhow!("Failed to get config dir path for '{file_id}'"))?;

    let file_path = project_dirs.config_dir().join(file_id);

    let mut file = fs::File::open(&file_path).await
        .with_context(|| format!("Couldn't open JSON config file at '{}'", file_path.display()))?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).await
        .with_context(|| format!("Couldn't read from JSON config file at '{}'", file_path.display()))?;

    let result = serde_json::from_str(&buf)
        .with_context(|| format!("Couldn't deserialize JSON config file at '{}'", file_path.display()))?;

    Ok(result)
}
