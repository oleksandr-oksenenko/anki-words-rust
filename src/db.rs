use std::fs;
use std::io::{ErrorKind, Read, Write};
use crate::model::{Book, Word};
use anyhow::{Context, Result};
use regex::Regex;

const DIR: &str = "data";

pub fn save_words(book_name: &str, words: &Vec<Word>) -> Result<()> {
    match fs::create_dir(DIR) {
        Err(err) =>
            if err.kind() != ErrorKind::AlreadyExists {
                return Err(err).with_context(|| format!("Failed to create dir '{DIR}'"))?;
            },
        _ => ()
    };

    let filename = get_filename(book_name);

    let mut file = fs::File::create(&filename)
        .with_context(|| format!("Failed to create file {filename}"))?;

    let json = serde_json::to_string(words)
        .with_context(|| format!("Failed to serialize words"))?;

    file.write_all(json.as_ref())
        .with_context(|| format!("Failed to write contents to the file"))?;

    Ok(())
}

pub fn get_words(book: &Book) -> Result<Vec<Word>> {
    let filename = get_filename(&book.title);
    let mut file = fs::File::open(filename)?;

    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let result = serde_json::from_str(buf.as_str())?;

    Ok(result)
}

fn get_filename(book_name: &str) -> String {
    let regex = Regex::new(r"[^a-z\s]").unwrap();

    let book_name = regex.replace_all(&book_name.to_lowercase(), "")
        .replace(" ", "_");

    format!("{DIR}/{book_name}.json")
}
