use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use anyhow::Result;
use clap::{Parser, Subcommand};
use env_logger::Env;
use futures::try_join;
use futures::future::join_all;
use inquire::{MultiSelect, Select, Text};
use itertools::{Itertools, process_results};
use log::{debug, error, info};

use crate::anki_connect::AnkiConnectClient;
use crate::google_translate::GoogleTranslate;
use crate::model::{Book, Word};
use crate::oxford_dict::OxfordDictClient;
use crate::readwise::ReadwiseClient;

mod anki_connect;
mod db;
mod google_auth;
mod google_translate;
mod model;
mod oxford_dict;
mod readwise;
mod util;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    ProcessWord { word: String },
    ProcessAll { force: Option<bool> },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    match &args.command {
        Commands::ProcessWord { word } => {
            debug!("Defining word: '{word}'");

            let mut word = Word::from_text(word);

            let result = WordProcessor::new().await?
                .process_word(&mut word).await;

            match result {
                Ok(word) => info!("Definition: {:?}", word),
                Err(err) => error!("Error: {err}")
            }
        }

        Commands::ProcessAll { force } => {
            debug!("Processing all words");

            let word_processor = WordProcessor::new().await?;

            match word_processor.process(force.unwrap_or(false)).await {
                Ok(_) => debug!("Finished."),
                Err(err) => error!("Global error: {}", err)
            }
        }
    }

    Ok(())
}

struct WordProcessor {
    readwise: ReadwiseClient,
    oxford_dict: OxfordDictClient,
    google_translate: GoogleTranslate,
    anki: AnkiConnectClient,
}

impl WordProcessor {
    pub async fn new() -> Result<WordProcessor> {
        let (readwise, oxford_dict, google_translate) = try_join!(
            ReadwiseClient::new(),
            OxfordDictClient::new(),
            GoogleTranslate::new()
        )?;

        Ok(WordProcessor {
            readwise,
            oxford_dict,
            google_translate,
            anki: AnkiConnectClient::new()?,
        })
    }

    pub async fn process(&self, force: bool) -> Result<()> {
        let mut books = self.readwise.get_books().await?;
        books.sort();
        let book = Self::select_book(books)?;

        let all_words = self.readwise.get_words(&book).await?;
        let processed_words = self.process_words_v2(&book, all_words, force).await?;

        db::save_words(&book.title, &processed_words).await?;

        self.anki.store_book(&book, &processed_words, force).await?;

        Ok(())
    }

    async fn process_words_v2(&self, book: &Book, all_words: Vec<Word>, force: bool) -> Result<Vec<Word>> {
        let (mut unprocessed_words, mut processed_words) = if !force {
            Self::partition_by_processed(&book, all_words).await?
        } else {
            (all_words, Vec::new())
        };

        let mut count = 0;
        while !unprocessed_words.is_empty() {
            let mut failed_words: Vec<Word> = Vec::new();

            for mut word in unprocessed_words {
                let result = self.process_word(&mut word).await;

                match result {
                    Ok(()) => processed_words.push(word),
                    Err(err) => {
                        error!("Failed to process word '{word}': {err}");
                        failed_words.push(word);
                    }
                };

                count += 1;
                if count % 10 == 0 {
                    info!("Processed {count} words");
                }
            }

            if !failed_words.is_empty() {
                unprocessed_words = Self::redact_words(failed_words)?;
            } else {
                break;
            }
        }

        Ok(processed_words)
    }

    pub async fn process_word(&self, word: &mut Word) -> Result<()> {
        let word_stem = self.oxford_dict.word_stem(&word.text).await
            .unwrap_or(word.text.to_owned());

        let (translation, defined_word) = try_join!(
            self.google_translate.translate(&word_stem),
            self.oxford_dict.definitions(&word_stem))?;

        word.text = defined_word.text;
        word.translation = Some(translation);
        word.definitions = defined_word.definitions;

        Ok(())
    }

    fn select_book(books: Vec<Book>) -> Result<Book> {
        Ok(Select::new("Select the book to import:", books)
            .with_page_size(20)
            .prompt()?)
    }

    fn redact_words(words: Vec<Word>) -> Result<Vec<Word>> {
        let selected = MultiSelect::new("Select words to redact: ", words)
            .prompt()?;

        let mut new_words = Vec::new();
        for mut word in selected {
            let redacted_text = Text::new("Redact: ")
                .with_initial_value(&word.text)
                .prompt()?;

            word.text = redacted_text;
            word.translation = None;
            word.definitions = None;

            new_words.push(word);
        }

        Ok(new_words)
    }

    async fn partition_by_processed<'a>(book: &Book, words: Vec<Word>) -> Result<(Vec<Word>, Vec<Word>)> {
        let mut cached_words = db::get_words(book).await?
            .into_iter()
            .map(|word| (word.original_text.clone(), word))
            .collect::<HashMap<String, Word>>();

        let (mut processed, mut unprocessed) = (Vec::new(), Vec::new());

        for word in words.into_iter() {
            if let Some(cached_word) = cached_words.remove(&word.original_text) {
                processed.push(cached_word);
            } else {
                unprocessed.push(word);
            }
        }

        Ok((unprocessed, processed))
    }
}
