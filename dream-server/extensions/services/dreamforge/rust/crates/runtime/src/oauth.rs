// OAuth is disabled in DreamForge — local auth via DreamServer is used instead.
//
// Struct definitions are retained so that downstream type signatures continue to
// compile, but every function now returns an error or a sensible default.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::OAuthConfig;

/// Persisted OAuth access token bundle used by the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scopes: Vec<String>,
}

/// PKCE verifier/challenge pair generated for an OAuth authorization flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceCodePair {
    pub verifier: String,
    pub challenge: String,
    pub challenge_method: PkceChallengeMethod,
}

/// Challenge algorithms supported by the local PKCE helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkceChallengeMethod {
    S256,
}

impl PkceChallengeMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::S256 => "S256",
        }
    }
}

/// Parameters needed to build an authorization URL for browser-based login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationRequest {
    pub authorize_url: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub state: String,
    pub code_challenge: String,
    pub code_challenge_method: PkceChallengeMethod,
    pub extra_params: BTreeMap<String, String>,
}

/// Request body for exchanging an OAuth authorization code for tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthTokenExchangeRequest {
    pub grant_type: &'static str,
    pub code: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub code_verifier: String,
    pub state: String,
}

/// Request body for refreshing an existing OAuth token set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthRefreshRequest {
    pub grant_type: &'static str,
    pub refresh_token: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}

/// Parsed query parameters returned to the local OAuth callback endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

// ---------------------------------------------------------------------------
// Stub impls — all construction helpers and IO functions return errors.
// ---------------------------------------------------------------------------

impl OAuthAuthorizationRequest {
    #[must_use]
    pub fn from_config(
        _config: &OAuthConfig,
        _redirect_uri: impl Into<String>,
        _state: impl Into<String>,
        _pkce: &PkceCodePair,
    ) -> Self {
        Self {
            authorize_url: String::new(),
            client_id: String::new(),
            redirect_uri: String::new(),
            scopes: Vec::new(),
            state: String::new(),
            code_challenge: String::new(),
            code_challenge_method: PkceChallengeMethod::S256,
            extra_params: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_extra_param(self, _key: impl Into<String>, _value: impl Into<String>) -> Self {
        self
    }

    #[must_use]
    pub fn build_url(&self) -> String {
        String::new()
    }
}

impl OAuthTokenExchangeRequest {
    #[must_use]
    pub fn from_config(
        _config: &OAuthConfig,
        _code: impl Into<String>,
        _state: impl Into<String>,
        _verifier: impl Into<String>,
        _redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            grant_type: "authorization_code",
            code: String::new(),
            redirect_uri: String::new(),
            client_id: String::new(),
            code_verifier: String::new(),
            state: String::new(),
        }
    }

    #[must_use]
    pub fn form_params(&self) -> BTreeMap<&str, String> {
        BTreeMap::new()
    }
}

impl OAuthRefreshRequest {
    #[must_use]
    pub fn from_config(
        _config: &OAuthConfig,
        _refresh_token: impl Into<String>,
        _scopes: Option<Vec<String>>,
    ) -> Self {
        Self {
            grant_type: "refresh_token",
            refresh_token: String::new(),
            client_id: String::new(),
            scopes: Vec::new(),
        }
    }

    #[must_use]
    pub fn form_params(&self) -> BTreeMap<&str, String> {
        BTreeMap::new()
    }
}

fn disabled_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "OAuth is disabled — DreamForge uses DreamServer local auth",
    )
}

pub fn generate_pkce_pair() -> io::Result<PkceCodePair> {
    Err(disabled_error())
}

pub fn generate_state() -> io::Result<String> {
    Err(disabled_error())
}

#[must_use]
pub fn code_challenge_s256(_verifier: &str) -> String {
    String::new()
}

#[must_use]
pub fn loopback_redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}/callback")
}

pub fn credentials_path() -> io::Result<PathBuf> {
    Err(disabled_error())
}

pub fn load_oauth_credentials() -> io::Result<Option<OAuthTokenSet>> {
    Ok(None)
}

pub fn save_oauth_credentials(_token_set: &OAuthTokenSet) -> io::Result<()> {
    Err(disabled_error())
}

pub fn clear_oauth_credentials() -> io::Result<()> {
    Ok(())
}

pub fn parse_oauth_callback_request_target(
    _target: &str,
) -> Result<OAuthCallbackParams, String> {
    Err("OAuth is disabled — DreamForge uses DreamServer local auth".to_string())
}

pub fn parse_oauth_callback_query(_query: &str) -> Result<OAuthCallbackParams, String> {
    Err("OAuth is disabled — DreamForge uses DreamServer local auth".to_string())
}
