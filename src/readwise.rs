use std::collections::HashMap;
use std::{thread, time};

use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use log::info;
use reqwest::header::HeaderValue;
use reqwest::{header, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use crate::{model, util};
use crate::model::Word;

pub struct ReadwiseClient {
    http: reqwest::Client,
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
    pub async fn new() -> Result<ReadwiseClient> {
        let token = Self::load_creds().await?.token;

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert("Content-Type", HeaderValue::from_str("application/json")?);
        default_headers.insert("Authorization", HeaderValue::from_str(&format!("Token {token}"))?);

        let http = reqwest::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(ReadwiseClient { http })
    }

    async fn load_creds() -> Result<Credentials> {
        util::load_json_config("readwise").await
            .with_context(|| "Failed to load JSON config for 'readwise'")
    }

    pub async fn get_words(&self, book: &model::Book) -> Result<Vec<Word>> {
        let pink_tag =
            |highlight: &BookHighlight| highlight.tags.iter().any(|tag| tag.name == "pink");

        Ok(self
            .get_highlights(book.id).await?
            .into_iter()
            .filter(pink_tag)
            .map(|highlight| highlight.text)
            .map(|word| ReadwiseClient::transform_word(&word))
            .unique()
            .map(|text| Word::from_text(&text))
            .collect())
    }

    fn transform_word(word: &str) -> String {
        let word = word.to_lowercase();
        let regex = regex::Regex::new("[^A-Za-z\\s-]").unwrap();
        regex.replace_all(&word, "").to_string()
    }

    pub async fn get_books(&self) -> Result<Vec<model::Book>> {
        Ok(self.get_list_data::<Book>("/books", &HashMap::new())
            .await?
            .into_iter()
            .map(|book| model::Book { id: book.id, author: book.author, title: book.title })
            .collect())
    }

    async fn get_highlights(&self, book_id: u64) -> Result<Vec<BookHighlight>> {
        let book_id_str = format!("{book_id}");
        let params = &HashMap::from([("book_id", book_id_str.as_str())]);
        self.get_list_data("/highlights", params).await
    }

    async fn get_list_data<T: DeserializeOwned>(
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

            let mut response: ListResponse<T> = self.make_request(path, &params).await?;
            page += 1;
            results.append(&mut response.results);

            match response.next {
                Some(_) => (),
                None => break Ok(results),
            }
        }
    }

    async fn make_request<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &HashMap<&str, &str>,
    ) -> Result<T> {
        for _ in 1..=3 {
            let url = format!("{URL}{path}");
            info!("Requesting {url}");

            let request = self.http.get(&url).query(params);

            let response = request.send().await?;

            if response.status() != StatusCode::TOO_MANY_REQUESTS {
                return Ok(response.json().await?);
            } else {
                let retry_after: u64 = response
                    .headers()
                    .get("Retry-After").ok_or(anyhow!("Tried to get Retry-After, but no header available"))?
                    .to_str()?
                    .parse::<u64>()?;
                tokio::time::sleep(time::Duration::from_secs(retry_after)).await;
            }
        }
        panic!("Failed to get response from readwise in time");
    }
}
