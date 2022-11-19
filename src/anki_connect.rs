use anyhow::{anyhow, bail, Result};
use futures::future::try_join_all;
use log::info;
use maud::html;
use reqwest::header::{self, HeaderValue};
use serde_json::{json, Value};

use crate::model::{Book, Word};

pub struct AnkiConnectClient {
    http: reqwest::Client,
}

const ENDPOINT_URL: &str = "http://localhost:8765";

impl AnkiConnectClient {
    pub fn new() -> Result<AnkiConnectClient> {
        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert("Content-Type", HeaderValue::from_str("application/json")?);

        let http = reqwest::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(AnkiConnectClient { http })
    }

    pub async fn store_book(&self, book: &Book, words: &Vec<Word>, force: bool) -> Result<()> {
        if force {
            self.delete_deck(&book.title).await?;
        }

        self.create_deck_if_not_exists(&book.title).await?;

        for word in words {
            self.add_word(&book.title, word).await?
        }

        Ok(())
    }

    async fn add_word(&self, deck_name: &str, word: &Word) -> Result<()> {
        let html = Self::generate_back_text_html(word)?;

        self.add_note(deck_name, &word.text, &html).await?;

        Ok(())
    }

    fn generate_back_text_html(word: &Word) -> Result<String> {
        let back_text = html! {
            p { (word.translation.as_ref().unwrap()) }

            ol type="I" {
                @for (category, definitions) in word.definitions.as_ref().unwrap() {
                    li {
                        p { (category) }

                        ol type="1" {
                            @for definition in definitions {
                                li {
                                    p { (definition.definition.as_ref().unwrap()) }

                                    ul {
                                        @for example in &definition.examples {
                                            li { (example) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }.into_string();

        Ok(back_text)
    }

    async fn add_note(&self, deck_name: &str, front_text: &str, back_text: &str) -> Result<()> {
        let request = json!({
            "version": 6,
            "action": "addNote",
            "params": {
                "note": {
                    "deckName": deck_name,
                    "modelName": "Basic",
                    "fields": {
                        "Front": front_text,
                        "Back": back_text
                    },
                    "options": {
                        "allowDuplicate": false,
                        "duplicateScope": "deck",
                        "duplicateScopeOptions": {
                            "deckName": deck_name
                        }
                    }
                }
            }
        });

        self.make_request(request).await?;

        Ok(())
    }

    async fn create_deck_if_not_exists(&self, deck_name: &str) -> Result<()> {
        let existing_decks = self.get_decks().await?;

        if !existing_decks.iter().any(|s| s == deck_name) {
            self.create_deck(deck_name).await?;
        }

        Ok(())
    }

    async fn get_decks(&self) -> Result<Vec<String>> {
        let request = json!({
            "version": 6,
            "action": "deckNames"
        });
        let text = self.make_request(request).await?;

        let response: Value = serde_json::from_str(&text).unwrap();
        let results = response.as_object().ok_or(anyhow!("Failed to map response to object"))?
            .get("result").ok_or(anyhow!("Failed to get 'result' field"))?
            .as_array().ok_or(anyhow!("Failed to map 'result' to array"))?
            .iter();

        let mut decks = Vec::new();
        for value in results {
            decks.push(
                value.as_str()
                    .ok_or(anyhow!("Failed to map 'result' array value to string"))?
                    .to_string()
            );
        }

        Ok(decks)
    }

    async fn create_deck(&self, deck_name: &str) -> Result<()> {
        let request = json!({
            "version": 6,
            "action": "createDeck",
            "params": {
                "deck": deck_name
            }
        });

        self.make_request(request).await?;

        Ok(())
    }

    async fn delete_deck(&self, deck_name: &str) -> Result<()> {
        let request = json!({
            "version": 6,
            "action": "deleteDecks",
            "params": {
                "decks": [deck_name],
                "cardsToo": true
            }
        });

        self.make_request(request).await?;

        Ok(())
    }

    async fn make_request(&self, request: Value) -> Result<String> {
        let response = self.http.post(ENDPOINT_URL)
            .body(request.to_string())
            .send().await?;

        if !response.status().is_success() {
            bail!("Request to Anki failed");
        }

        Ok(response.text().await?)
    }
}
