extern crate env_logger;

use std::fmt::{Display, Formatter};
use anyhow::Result;
use clap::{Parser, Subcommand};
use env_logger::Env;
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
    ProcessWord { word: String },
    ProcessAll { force: Option<bool> }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    match &args.command {
        Commands::ProcessWord { word } => {
            debug!("Defining word: '{word}'");

            let mut word = Word::from_text(word);

            WordProcessor::new()?
                .process_word(&mut word)?;

            info!("Definition: {:?}", word);
        },

        Commands::ProcessAll { force } => {
            debug!("Processing all words");

            let word_processor = WordProcessor::new()?;

            match word_processor.process(force.unwrap_or(false)) {
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

    pub fn process(&self, force: bool) -> Result<()> {
        let mut books = self.readwise.get_books()?;
        books.sort();

        let book = Self::select_book(books)?;
        let mut words = self.readwise.get_words(&book)?;
        let mut processed_words: Vec<Word> = Vec::new();

        if !force {
            let mut cached_translations = db::get_words(&book)?;

            let words_count = words.len();
            words = words.into_iter()
                .filter(|word| !cached_translations.iter().any(|cached_word| cached_word.original_text == word.original_text))
                .collect();

            processed_words.append(& mut cached_translations);

            info!("{} are already processed, processing {} others", words_count - words.len() ,words.len());
        }

        let mut count = 0;
        while !words.is_empty() {
            let mut failed_words: Vec<Word> = Vec::new();

            for mut word in words {
                let processed_word = self.process_word(&mut word);

                match processed_word {
                    Ok(()) => processed_words.push(word),
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

    pub fn process_word(&self, word: &mut Word) -> Result<()> {
        let word_stem = self.oxford_dict.word_stem(&word.text)
            .unwrap_or(word.text.to_owned());

        let translation = self.google_translate.translate(&word_stem)?;
        let defined_word = self.oxford_dict.definitions(&word_stem)?;

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
        impl Display for Word {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.text)
            }
        }

        let words_to_redact = MultiSelect::new("Select words to redact: ", words)
            .prompt()?;

        let mut new_words = Vec::new();
        for mut word in words_to_redact {
            let redacted_text = Text::new("Redact: ")
                .with_initial_value(&word.text)
                .prompt()?;

            word.text = redacted_text;

            new_words.push(word);
        }

        Ok(new_words)
    }
}
