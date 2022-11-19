use std::{thread, time};
use std::collections::HashMap;
use std::str::FromStr;

use futures::future::{BoxFuture, FutureExt};

use anyhow::{anyhow, bail, Context, Result};
use itertools::Either::{Left, Right};
use itertools::Itertools;
use log::{info, warn};
use reqwest::{header, StatusCode};
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

use crate::model::{Definition, DefinitionCategory, DefinitionsEntry, Word};
use crate::util;

pub struct OxfordDictClient {
    http: reqwest::Client,
}

#[derive(Debug)]
pub enum OxfordClientError {
    CompositeError(Vec<anyhow::Error>),
}

impl std::error::Error for OxfordClientError {}

impl std::fmt::Display for OxfordClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OxfordClientError::CompositeError(errors) => {
                let error_str = errors.iter().join("\n");

                write!(f, "{error_str}")?;
            }
        }

        Ok(())
    }
}

enum MappingResult<T> {
    Result(T),
    OtherSources(Vec<String>),
}

const URL: &str = "https://od-api.oxforddictionaries.com/api/v2";

#[derive(Deserialize, Serialize)]
struct LemmasResponse {
    results: Option<Vec<LemmasResults>>,
}

#[derive(Deserialize, Serialize)]
struct LemmasResults {
    #[serde(rename = "lexicalEntries")]
    lexical_entries: Vec<LemmasLexicalEntry>,
}

#[derive(Deserialize, Serialize)]
struct LemmasLexicalEntry {
    #[serde(rename = "inflectionOf")]
    inflection_of: Vec<CommonTextEntry>,

    #[serde(rename = "lexicalCategory")]
    lexical_category: CommonTextEntry,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct EntriesResponse {
    results: Option<Vec<EntriesResults>>,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct EntriesResults {
    #[serde(rename = "lexicalEntries")]
    lexical_entries: Vec<EntriesLexicalEntry>,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct EntriesLexicalEntry {
    entries: Vec<EntriesEntry>,
    #[serde(rename = "lexicalCategory")]
    lexical_category: CommonTextEntry,
    #[serde(rename = "derivativeOf")]
    derivative_of: Option<Vec<CommonTextEntry>>,
}

#[derive(Serialize, Deserialize)]
#[derive(Debug)]
struct EntriesEntry {
    senses: Vec<EntriesSense>,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct EntriesSense {
    definitions: Option<Vec<String>>,
    examples: Option<Vec<CommonTextEntry>>,
    #[serde(rename = "shortDefinitions")]
    short_definitions: Option<Vec<String>>,
    subsenses: Option<Vec<EntriesSense>>,
    #[serde(rename = "crossReferences")]
    cross_references: Option<Vec<CommonTextEntry>>,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct CommonTextEntry {
    text: String,
}

#[derive(Deserialize, Serialize)]
#[derive(Debug)]
struct Credentials {
    app_id: String,
    app_key: String
}

impl OxfordDictClient {
    pub async fn new() -> Result<OxfordDictClient> {
        let creds = Self::load_creds().await?;

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert("Accept", HeaderValue::from_str("application/json")?);
        default_headers.insert("Content-Type", HeaderValue::from_str("application/json")?);
        default_headers.insert("App-Id", HeaderValue::from_str(&creds.app_id)?);
        default_headers.insert("App-Key", HeaderValue::from_str(&creds.app_key)?);

        let http = reqwest::Client::builder()
            .default_headers(default_headers)
            .connection_verbose(true)
            .build()?;

        Ok(OxfordDictClient { http })
    }

    async fn load_creds() -> Result<Credentials> {
        util::load_json_config("oxford_dict").await
            .with_context(|| format!("Failed to get credentials for oxford dict client"))
    }

    pub async fn word_stem(&self, word: &str) -> Result<String> {
        self.lemmas(word).await
    }

    pub async fn definitions(&self, word_stem: &str) -> Result<Word> {
        let en_us_entries = self.entries(word_stem, "en-us").await;

        if en_us_entries.is_ok() {
            return en_us_entries.map(|e| self.process_entries(e));
        }

        let en_gb_entries = self.entries(word_stem, "en-gb").await;
        if en_gb_entries.is_ok() {
            return en_gb_entries.map(|e| self.process_entries(e));
        }

        let errors = vec![en_us_entries.err().unwrap(), en_gb_entries.err().unwrap()];

        return Err(OxfordClientError::CompositeError(errors))?;
    }

    fn process_entries(&self, entries: (String, Vec<DefinitionsEntry>)) -> Word {
        let mut definitions = HashMap::new();

        entries.1.into_iter()
            .map(|def_entry| (def_entry.category, def_entry.definitions))
            .for_each(|(key, ref mut val)| {
                definitions.entry(key).or_insert_with(Vec::new).append(val);
            });

        Word {
            text: entries.0.to_owned(),
            original_text: entries.0,
            translation: None,
            definitions: Some(definitions),
        }
    }

    fn entries<'a>(&'a self, word_id: &'a str, lang: &'a str) -> BoxFuture<Result<(String, Vec<DefinitionsEntry>)>> {
        async move {
            let response: EntriesResponse = self.make_request(&format!("/entries/{lang}/{word_id}")).await?;

            if response.results.is_none() {
                bail!("Entries results array is empty");
            }

            let (successes, failures): (Vec<_>, Vec<_>) = response.results.unwrap().into_iter()
                .flat_map(|result| result.lexical_entries)
                .map(|lexical_entry| OxfordDictClient::map_lexical_entry(word_id, lexical_entry))
                .partition_result();

            if !failures.is_empty() {
                return Err(OxfordClientError::CompositeError(failures))?;
            }

            let (results, other_sources): (Vec<_>, Vec<_>) = successes.into_iter()
                .partition_map(|mapping_result| match mapping_result {
                    MappingResult::Result(r) => Left(r),
                    MappingResult::OtherSources(os) => Right(os)
                });
            let other_sources: Vec<String> = other_sources.into_iter().flatten().collect();

            return if !results.is_empty() {
                if !other_sources.is_empty() {
                    warn!("other sources are not empty for '{word_id}': {:?}", other_sources)
                }
                Ok((word_id.to_owned(), results))
            } else if !other_sources.is_empty() {
                //TODO: handle multiple other sources?
                let source = other_sources.first().unwrap();
                info!("Failed to get definition for '{word_id}', getting it from other source: '{source}'");
                self.entries(source, lang).await
            } else {
                Err(anyhow!("Definition entries and other sources are empty for '{word_id}'"))
            };
        }.boxed()
    }

    fn map_lexical_entry(word_id: &str, lexical_entry: EntriesLexicalEntry) -> Result<MappingResult<DefinitionsEntry>> {
        let lexical_category = lexical_entry.lexical_category.text.trim().to_lowercase();
        let category = DefinitionCategory::from_str(&lexical_category)
            .with_context(|| format!("Failed to convert lexical category from '{lexical_category}'"))?;

        let (definitions, other_sources): (Vec<_>, Vec<_>) = lexical_entry.entries.into_iter()
            .flat_map(|entry| entry.senses)
            .flat_map(|sense| OxfordDictClient::build_definitions(sense))
            .partition_map(|mapping_result| match mapping_result {
                MappingResult::Result(r) => Left(r),
                MappingResult::OtherSources(os) => Right(os)
            });

        let definitions: Vec<Definition> = definitions.into_iter()
            .filter(|def| !def.definition.is_none())
            .collect();
        let mut other_sources: Vec<String> = other_sources.into_iter().flatten().collect();

        let mut derivative_of: Vec<String> = lexical_entry.derivative_of
            .map(|derivative_of| derivative_of.into_iter().map(|dof| dof.text).collect())
            .unwrap_or_default();

        return if !definitions.is_empty() {
            if !other_sources.is_empty() {
                warn!("other sources are not empty for {word_id}: {:?}", other_sources);
            }
            Ok(MappingResult::Result(DefinitionsEntry { definitions, category }))
        } else if !other_sources.is_empty() || !derivative_of.is_empty() {
            other_sources.append(&mut derivative_of);

            Ok(MappingResult::OtherSources(other_sources))
        } else {
            Err(anyhow!("Failed to find definitions or other sources for word '{word_id}' and category '{category}'"))
        };
    }

    fn build_definitions(mut sense: EntriesSense) -> Vec<MappingResult<Definition>> {
        let mut sub_senses_definitions = sense.subsenses.take().unwrap_or_default()
            .into_iter()
            .map(|ss| OxfordDictClient::build_definition(ss))
            .collect::<Vec<_>>();

        let main_sense_definition = OxfordDictClient::build_definition(sense);

        sub_senses_definitions.insert(0, main_sense_definition);
        sub_senses_definitions
    }

    fn build_definition(sense: EntriesSense) -> MappingResult<Definition> {
        let short_definitions = sense.short_definitions.unwrap_or_default();
        let definitions = sense.definitions.unwrap_or_default();

        let definition = short_definitions.first()
            .or(definitions.first())
            .cloned();

        let examples = sense.examples.unwrap_or_default()
            .iter()
            .map(|example| example.text.clone())
            .collect();

        let cross_references = sense.cross_references.unwrap_or_default();

        return if definition.is_none() && !cross_references.is_empty() {
            let cross_references = cross_references.iter().map(|cte| cte.text.to_lowercase()).collect();
            MappingResult::OtherSources(cross_references)
        } else {
            MappingResult::Result(Definition { definition, examples })
        };
    }

    async fn lemmas(&self, word: &str) -> Result<String> {
        let response: LemmasResponse = self.make_request(&format!("/lemmas/en/{word}")).await?;

        if response.results.is_none() {
            bail!("Lemmas results array is empty, bailing early")
        }

        let inflections: Vec<String> = response.results.unwrap()
            .into_iter()
            .flat_map(|result| result.lexical_entries)
            .flat_map(|le| le.inflection_of)
            .map(|inf| inf.text)
            .unique()
            .collect();

        if inflections.len() > 1 {
            inflections.iter()
                .find(|inflection| inflection.as_str() == word)
                .or(inflections.iter().next())
                .cloned()
                .ok_or(anyhow!("No inflections found for {word}"))
        } else {
            inflections.into_iter()
                .next()
                .ok_or(anyhow!("No inflections found"))
        }
    }

    async fn make_request<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        for _ in 1..3 {
            let url = format!("{URL}{path}");
            info!("Requesting {url}");
            let response = self.http.get(&url).send().await?;

            if response.status() != StatusCode::TOO_MANY_REQUESTS {
                let result = response.json::<T>().await?;
                return Ok(result);
            } else {
                let retry_after: u64 = response
                    .headers()
                    .get("Retry-After").ok_or_else(|| anyhow!("Failed to get Retry-After header"))?
                    .to_str()?
                    .parse::<u64>()?;
                info!("Waiting {} seconds...", retry_after);
                tokio::time::sleep(time::Duration::from_secs(retry_after)).await;
            }
        }
        bail!("Failed to get response from Oxford dict in time");
    }
}
