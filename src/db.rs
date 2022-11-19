use std::io::ErrorKind;
use tokio::fs;
use crate::model::{Book, Word};
use anyhow::{Context, Result};
use log::info;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DIR: &str = "data";

pub async fn save_words(book_name: &str, words: &Vec<Word>) -> Result<()> {
    match fs::create_dir(DIR).await {
        Err(err) =>
            if err.kind() != ErrorKind::AlreadyExists {
                return Err(err).with_context(|| format!("Failed to create dir '{DIR}'"))?;
            },
        _ => ()
    };

    let filename = get_filename(book_name);

    let mut file = fs::File::create(&filename).await
        .with_context(|| format!("Failed to create file {filename}"))?;

    let json = serde_json::to_string(words)
        .with_context(|| format!("Failed to serialize words"))?;

    file.write_all(json.as_ref()).await
        .with_context(|| format!("Failed to write contents to the file"))?;

    Ok(())
}

pub async fn get_words(book: &Book) -> Result<Vec<Word>> {
    let filename = get_filename(&book.title);
    let file_open_result = fs::File::open(&filename).await;

    if file_open_result.is_err() {
        let error = file_open_result.err().unwrap();
        return if error.kind() == ErrorKind::NotFound {
            info!("Words file '{filename}' doesn't exist");
            Ok(Vec::new())
        } else {
            Err(error)
                .with_context(|| format!("Couldn't open data file at '{filename}'"))
        }
    }
    let mut file = file_open_result.unwrap();

    let mut buf = String::new();
    file.read_to_string(&mut buf).await
        .with_context(|| format!("Couldn't read words from file at '{filename}'"))?;

    let result = serde_json::from_str(buf.as_str())
        .with_context(|| format!("Couldn't deserialize words from file at '{filename}'"))?;

    Ok(result)
}

fn get_filename(book_name: &str) -> String {
    let regex = Regex::new(r"[^a-z\s]").unwrap();

    let book_name = regex.replace_all(&book_name.to_lowercase(), "")
        .replace(" ", "_");

    format!("{DIR}/{book_name}.json")
}
