use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Display;
use serde::{Serialize, Deserialize};
use strum::{Display, EnumString};
use std::string::ToString;
use maud::Render;

#[derive(Debug, Serialize, Deserialize)]
pub struct Book {
    pub id: u64,
    pub title: String,
    pub author: Option<String>,
    pub words: Vec<Word>
}

impl Display for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.author {
            None => write!(f, "N/A: {}", self.title),
            Some(author) => write!(f, "{}: {}", author, self.title),
        }
    }
}

impl PartialEq for Book {
    fn eq(&self, other: &Self) -> bool {
        self.author == other.author && self.title == other.title
    }
}

impl Eq for Book {}

impl PartialOrd for Book {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.author.is_some() && other.author.is_some() {
            let author_cmp = self.author.cmp(&other.author);

            if author_cmp == Ordering::Equal {
                Some(self.title.to_lowercase().cmp(&other.title.to_lowercase()))
            } else {
                Some(author_cmp)
            }
        } else if self.author.is_some() {
            Some(Ordering::Less)
        } else if other.author.is_some() {
            Some(Ordering::Greater)
        } else {
            Some(self.title.to_lowercase().cmp(&other.title.to_lowercase()))
        }
    }
}

impl Ord for Book {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub type Definitions = HashMap<DefinitionCategory, Vec<Definition>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub original_text: String,
    pub translation: Option<String>,
    pub definitions: Option<Definitions>
}

impl Word {
    pub fn from_text(text: &str) -> Word {
        Word {
            text: text.to_owned(),
            original_text: text.to_owned(),
            translation: None,
            definitions: None
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DefinitionsEntry {
    pub definitions: Vec<Definition>,
    pub category: DefinitionCategory
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Definition {
    pub definition: Option<String>,
    pub examples: Vec<String>
}

#[derive(Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum DefinitionCategory {
    Noun,
    Verb,
    Adjective,
    Adverb,
    Preposition,
    Interjection,
    Idiomatic,
    Pronoun
}

impl Render for DefinitionCategory {
    fn render_to(&self, buffer: &mut String) {
        buffer.push_str(&self.to_string())
    }
}
