use reqwest::header::{self, HeaderValue};
use serde_json::{json, Value};
use anyhow::{anyhow, bail, Result};
use maud::html;
use crate::model::{Book, Word};

pub struct AnkiConnectClient {
    http: reqwest::blocking::Client
}

const ENDPOINT_URL: &str = "http://localhost:8765";

impl AnkiConnectClient {
    pub fn new() -> Result<AnkiConnectClient> {
        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert( "Content-Type", HeaderValue::from_str("application/json")?);

        let http = reqwest::blocking::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(AnkiConnectClient { http })
    }

    pub fn store_book(&self, book: &Book, words: &Vec<Word>) -> Result<()> {
        self.create_deck_if_not_exists(&book.title)?;

        for word in words {
            self.add_word(&book.title, word)?;
        }

        Ok(())
    }

    fn add_word(&self, deck_name: &str, word: &Word) -> Result<()> {
        let html = Self::generate_back_text_html(word)?;

        self.create_deck_if_not_exists(deck_name)?;
        self.add_note(deck_name, &word.text, &html)?;

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

    fn add_note(&self, deck_name: &str, front_text: &str, back_text: &str) -> Result<()> {
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

        let response = self.http.post(ENDPOINT_URL)
            .body(request.to_string())
            .send()?;

        if !response.status().is_success() {
            bail!("Failed to add note to Anki");
        }

        Ok(())
    }

    fn create_deck_if_not_exists(&self, deck_name: &str) -> Result<()> {
        let existing_decks = self.get_decks()?;

        if !existing_decks.iter().any(|s| s == deck_name) {
            self.create_deck(deck_name)?;
        }

        Ok(())
    }

    fn get_decks(&self) -> Result<Vec<String>> {
        let request = json!({
            "version": 6,
            "action": "deckNames"
        });
        let text = self.http.post(ENDPOINT_URL)
            .body(request.to_string())
            .send()?
            .text()?;

        let response: Value = serde_json::from_str(&text).unwrap();
        let results = response.as_object().ok_or(anyhow!("Failed to map response to object"))?
            .get("result").ok_or(anyhow!("Failed to get 'result' field"))?
            .as_array().ok_or(anyhow!("Failed to map 'result' to array"))?
            .iter();

        let mut decks = Vec::new();
        for value in results {
            decks.push(
                value.as_str().ok_or(anyhow!("Failed to map 'result' array value to string"))?
                    .to_string()
            );
        }

        Ok(decks)
    }

    fn create_deck(&self, deck_name: &str) -> Result<()> {
        let request = json!({
            "version": 6,
            "action": "createDeck",
            "params": {
                "deck": deck_name
            }
        });

        let response = self.http.post(ENDPOINT_URL)
            .body(request.to_string())
            .send()?;

        if !response.status().is_success() {
            bail!("Failed to create Anki deck");
        }

        Ok(())
    }
}
