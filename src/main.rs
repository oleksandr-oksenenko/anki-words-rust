extern crate env_logger;

use anyhow::Result;
use clap::{Parser, Subcommand};
use inquire::{MultiSelect, Select, Text};
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
    command: Commands
}

#[derive(Subcommand, Debug)]
enum Commands {
    DefineWord { word: String },
    ProcessAll
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    match &args.command {
        Commands::DefineWord { word } => {
            debug!("Defining word: '{word}'");
            let word = WordProcessor::new()?
                .process_word(&word)?;

            info!("Definition: {:?}", word);
        },

        Commands::ProcessAll => {
            debug!("Processing all words");

            let word_processor = WordProcessor::new()?;
            match word_processor.process() {
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
    anki: AnkiConnectClient
}

impl WordProcessor {
    pub fn new() -> Result<WordProcessor> {
        Ok(WordProcessor {
            readwise: ReadwiseClient::new()?,
            oxford_dict: OxfordDictClient::new()?,
            google_translate: GoogleTranslate::new()?,
            anki: AnkiConnectClient::new()?
        })
    }

    pub fn process(&self) -> Result<()> {
        let mut books = self.readwise.get_books()?;
        books.sort();

        let book = Self::select_book(books)?;
        let mut words = self.readwise.get_words(&book)?;

        let mut processed_words: Vec<Word> = Vec::new();

        let mut count = 0;
        while !words.is_empty() {
            let mut failed_words: Vec<String> = Vec::new();

            for word in words {
                let processed_word = self.process_word(&word);

                match processed_word {
                    Ok(processed_word) => processed_words.push(processed_word),
                    Err(err) => {
                        error!("Failed to process '{word}': {err}");
                        failed_words.push(word);
                    }
                }
                if count % 10 == 0 {
                    debug!("Processed {count} words");
                }
                count += 1;
            }

            if !failed_words.is_empty() {
                words = Self::redact_words(failed_words)?;
            } else {
                break
            }
        }

        db::save_words(&book.title, &processed_words)?;

        self.anki.store_book(&book, &processed_words)?;

        Ok(())
    }

    pub fn process_word(&self, word: &str) -> Result<Word> {
        let word_stem = self.oxford_dict.word_stem(word)
            .unwrap_or(word.to_string());

        let translation = self.google_translate.translate(&word_stem)?;
        let (word_id, definitions) = self.oxford_dict.definitions(&word_stem)?;

        Ok(Word {
            text: word_id,
            translation: Some(translation),
            definitions_entries: Some(definitions),
        })
    }

    fn select_book(books: Vec<Book>) -> Result<Book> {
        Ok(Select::new("Select the book to import:", books)
            .with_page_size(20)
            .prompt()?)
    }

    fn redact_words(words: Vec<String>) -> Result<Vec<String>> {
        let words_to_redact = MultiSelect::new("Select words to redact: ", words)
            .prompt()?;

        let mut new_words = Vec::new();
        for word in words_to_redact {
            let redacted_word = Text::new("Redact: ")
                .with_initial_value(&word)
                .prompt()?;

            new_words.push(redacted_word);
        }

        Ok(new_words)
    }
}
