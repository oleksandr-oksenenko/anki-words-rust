use std::collections::HashMap;
use std::{thread, time};

use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use reqwest::header::HeaderValue;
use reqwest::{header, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use crate::{model, util};
use crate::model::Word;

pub struct ReadwiseClient {
    http: reqwest::blocking::Client,
}

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    next: Option<String>,
    results: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct Book {
    id: u64,
    title: String,
    author: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BookHighlight {
    text: String,
    tags: Vec<BookTag>,
}

#[derive(Debug, Deserialize)]
struct BookTag {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Credentials {
    token: String
}

const URL: &str = "https://readwise.io/api/v2";

impl ReadwiseClient {
    pub fn new() -> Result<ReadwiseClient> {
        let token = Self::load_creds()?.token;

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert("Content-Type", HeaderValue::from_str("application/json")?);
        default_headers.insert("Authorization", HeaderValue::from_str(&format!("Token {token}"))?);

        let http = reqwest::blocking::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(ReadwiseClient { http })
    }

    fn load_creds() -> Result<Credentials> {
        util::load_json_config("readwise")
            .with_context(|| "Failed to load JSON config for 'readwise'")
    }

    pub fn get_words(&self, book: &model::Book) -> Result<Vec<Word>> {
        let pink_tag =
            |highlight: &BookHighlight| highlight.tags.iter().any(|tag| tag.name == "pink");

        Ok(self
            .get_highlights(book.id)?
            .into_iter()
            .filter(pink_tag)
            .map(|highlight| highlight.text)
            .map(|word| ReadwiseClient::transform_word(&word))
            .unique()
            .map(|text| Word {
                text: text.to_owned(),
                original_text: text.to_owned(),
                translation: None,
                definitions_entries: None
            })
            .collect())
    }

    fn transform_word(word: &str) -> String {
        let word = word.to_lowercase();
        let regex = regex::Regex::new("[^A-Za-z\\s-]").unwrap();
        regex.replace_all(&word, "").to_string()
    }

    pub fn get_books(&self) -> Result<Vec<model::Book>> {
        Ok(self.get_list_data::<Book>("/books", &HashMap::new())?
            .into_iter()
            .map(|book| model::Book { id: book.id, author: book.author, title: book.title, words: Vec::new() })
            .collect())
    }

    fn get_highlights(&self, book_id: u64) -> Result<Vec<BookHighlight>> {
        let book_id_str = format!("{book_id}");
        let params = &HashMap::from([("book_id", book_id_str.as_str())]);
        self.get_list_data("/highlights", params)
    }

    fn get_list_data<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &HashMap<&str, &str>,
    ) -> Result<Vec<T>> {
        let mut page = 1;
        let mut results: Vec<T> = Vec::new();

        loop {
            let page_str = format!("{page}");
            let mut params = params.clone();
            params.insert("page", &page_str);
            params.insert("page_size", "1000");

            let mut response: ListResponse<T> = self.make_request(path, &params)?;
            page += 1;
            results.append(&mut response.results);

            match response.next {
                Some(_) => (),
                None => break Ok(results),
            }
        }
    }

    fn make_request<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &HashMap<&str, &str>,
    ) -> Result<T> {
        for _ in 1..=3 {
            let request = self.http.get(&format!("{URL}{path}")).query(params);

            let response = request.send()?;

            if response.status() != StatusCode::TOO_MANY_REQUESTS {
                let result = response.json::<T>()?;
                return Ok(result);
            } else {
                let retry_after: u64 = response
                    .headers()
                    .get("Retry-After").ok_or(anyhow!("Tried to get Retry-After, but no header available"))?
                    .to_str()?
                    .parse::<u64>()?;
                thread::sleep(time::Duration::from_millis(retry_after));
            }
        }
        panic!("Failed to get response from readwise in time");
    }
}
