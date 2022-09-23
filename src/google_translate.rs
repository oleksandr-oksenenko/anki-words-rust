use serde::{Serialize, Deserialize};

use anyhow::{anyhow, Context};
use anyhow::Result;
use crate::google_auth::TokenManager;
use reqwest::header;
use reqwest::header::HeaderValue;

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";
const SCOPE: &str = "https://www.googleapis.com/auth/cloud-translation";

pub struct GoogleTranslate {
    http: reqwest::blocking::Client,
}

#[derive(Serialize, Deserialize)]
struct Request {
    q: String,
    source: String,
    target: String,
    format: String,
}

#[derive(Serialize, Deserialize)]
struct Response {
    data: TranslationsResponse,
}

#[derive(Serialize, Deserialize)]
struct TranslationsResponse {
    translations: Vec<TranslationResponse>,
}

#[derive(Serialize, Deserialize)]
struct TranslationResponse {
    #[serde(rename = "translatedText")]
    translated_text: Option<String>,
}

impl Request {
    fn new(query: &str) -> Request {
        Request {
            q: query.to_string(),
            source: "en".to_string(),
            target: "ru".to_string(),
            format: "text".to_string(),
        }
    }
}

impl GoogleTranslate {
    pub fn new() -> Result<GoogleTranslate> {
        let scopes = [SCOPE];
        let token_manager = TokenManager::new(&scopes);
        let token = token_manager
            .with_context(|| "Failed to create Google Token Manager")?
            .token()?;

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert("Content-Type", HeaderValue::from_str("application/json")?);
        default_headers.insert("Authorization", HeaderValue::from_str(&token)?);

        let http = reqwest::blocking::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(GoogleTranslate { http })
    }

    pub fn translate(&self, query: &str) -> Result<String> {
        let request = Request::new(query);
        let body = serde_json::to_string(&request)?;

        let response: Response = self.http.post(ENDPOINT)
            .body(body)
            .send()?
            .json()?;

        let translation = response.data.translations.into_iter().next();
        translation.map(|t| t.translated_text)
            .flatten()
            .ok_or(anyhow!("No translation"))
    }
}
