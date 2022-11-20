use std::fmt;

use anyhow::{Context, Result};
use chrono::DateTime;
use chrono::offset::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util;

const AUTH_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

#[derive(Serialize, Deserialize)]
pub struct ApplicationCredentials {
    #[serde(rename = "type")]
    pub cred_type: String,
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
}

#[derive(Debug, PartialEq, Clone)]
enum TokenValue {
    Bearer(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    value: TokenValue,
    expiry: DateTime<Utc>,
}

impl fmt::Display for TokenValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenValue::Bearer(token) => write!(f, "Bearer {token}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AuthResponse {
    access_token: String,
}

pub struct TokenManager {
    http: reqwest::Client,
    scopes: String,
    creds: ApplicationCredentials,
    current_token: Option<Token>,
}

impl TokenManager {
    pub async fn new(scopes: &[&str]) -> Result<TokenManager> {
        let creds = TokenManager::load_creds().await?;

        let http = reqwest::Client::builder()
            .connection_verbose(true)
            .build()?;

        Ok(TokenManager {
            creds,
            http,
            scopes: scopes.join(" "),
            current_token: None,
        })
    }

    async fn load_creds() -> Result<ApplicationCredentials> {
        util::load_json_config("google_translate").await
            .with_context(|| "Failed to load JSON config for 'google-translate'")
    }

    pub async fn token(&mut self) -> Result<String> {
        let hour = chrono::Duration::minutes(45);
        let current_time = Utc::now();

        match self.current_token {
            Some(ref token) if token.expiry >= current_time => Ok(token.value.to_string()),
            _ => {
                let expiry = current_time + hour;
                let claims = json!({
                    "iss": self.creds.client_email.as_str(),
                    "scope": self.scopes.as_str(),
                    "aud": AUTH_ENDPOINT,
                    "exp": expiry.timestamp(),
                    "iat": current_time.timestamp()
                });

                let token = jwt::encode(
                    &jwt::Header::new(jwt::Algorithm::RS256),
                    &claims,
                    &jwt::EncodingKey::from_rsa_pem(self.creds.private_key.as_bytes())?,
                )?;

                let form = format!(
                    "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={}",
                    token.as_str()
                );

                let response: AuthResponse = self.http.post(AUTH_ENDPOINT)
                    .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(form)
                    .send()
                    .await?
                    .json()
                    .await?;

                let value = TokenValue::Bearer(response.access_token);
                let token = value.to_string();
                self.current_token = Some(Token { expiry, value });

                Ok(token)
            }
        }
    }
}
