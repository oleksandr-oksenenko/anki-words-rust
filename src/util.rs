use std::fs;
use serde::de::DeserializeOwned;
use std::io::Read;
use anyhow::{anyhow, Result};

pub fn load_json_config<T: DeserializeOwned>(file_id: &str) -> Result<T> {
    let project_dirs = directories::ProjectDirs::from("net", "oksenenko", "anki-words-importer")
        .ok_or(anyhow!("Failed to get config dir path for '{file_id}'"))?;

    let file_path = project_dirs.config_dir().join(file_id);

    println!("Trying to load JSON config from '{}'", file_path.display());

    let mut file = fs::File::open(file_path)?;

    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let result = serde_json::from_str(&buf)?;

    Ok(result)
}
