#![allow(async_fn_in_trait)]

use famedly_rust_utils::BaseUrl;
use serde::{Deserialize, Serialize};

use crate::{Action, LoadedScript};

pub trait ZitadelHandle: Send + Sync {
    type Err: Send + Sync;
    async fn search_actions_by_name(&self, name: &str) -> Result<Option<ActionSearch>, Self::Err>;
    async fn create_action(&self, action: ActionCreate) -> Result<String, Self::Err>;
    async fn update_action(&self, id: &str, action: ActionUpdate) -> Result<(), Self::Err>;
    async fn delete_action(&self, id: &str) -> Result<(), Self::Err>;

    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
    ) -> Result<(), Self::Err>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSearch {
    pub id: String,
    pub name: String,
    pub timeout: Option<String>,
    pub allowed_to_fail: Option<bool>,
    pub script: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionCreate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_to_fail: Option<bool>,
    pub script: String,
}

impl ActionCreate {
    #[must_use]
    pub fn new(name: String, action: Action<LoadedScript>) -> Self {
        Self {
            name,
            timeout: action.timeout,
            allowed_to_fail: action.allowed_to_fail,
            script: action.script,
        }
    }
}

// TODO: skip ser if none
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionUpdate {
    // pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_to_fail: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

impl ActionUpdate {
    #[must_use]
    pub fn new(action: Action<LoadedScript>) -> Self {
        Self {
            // name: None,
            timeout: action.timeout,
            allowed_to_fail: action.allowed_to_fail,
            script: Some(action.script),
        }
    }
}

#[must_use]
pub fn action_is_same(action: &Action<LoadedScript>, a: &ActionSearch) -> bool {
    action.timeout == a.timeout
        && action.allowed_to_fail == a.allowed_to_fail
        && action.script == a.script
}

#[derive(Debug)]
pub struct SimpleZitadelClient {
    client: reqwest::Client,
    url: BaseUrl,
}

use reqwest::header::{HeaderMap, AUTHORIZATION};

#[derive(Debug, thiserror::Error)]
pub enum SimpleZitadelClientCreationError {
    #[error("http request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    HeaderParsing(#[from] reqwest::header::InvalidHeaderValue),
}

impl SimpleZitadelClient {
    pub fn new(
        url: BaseUrl,
        token: String,
        org_id: Option<String>,
    ) -> Result<Self, SimpleZitadelClientCreationError> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(1))
                .default_headers({
                    let mut headers = HeaderMap::new();
                    headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
                    if let Some(org_id) = org_id {
                        headers.insert("x-zitadel-orgid", org_id.parse()?);
                    }
                    headers
                })
                .build()?,
            url,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SimpleZitadelClientError {
    #[error("serde failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("http request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("url parsing failed: {0}")]
    Url(#[from] url::ParseError),
}

#[derive(Serialize)]
struct EmptyBody {}

impl ZitadelHandle for SimpleZitadelClient {
    type Err = SimpleZitadelClientError;
    async fn search_actions_by_name(&self, name: &str) -> Result<Option<ActionSearch>, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            result: Vec<ActionSearch>,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions/_search")?)
            .json(&serde_json::json!({
              "query": { "limit": 1 },
              "queries": [
                {
                  "actionNameQuery": {
                    "name": name,
                    "method": "TEXT_QUERY_METHOD_EQUALS"
                  },
                }
              ]
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await?
            .result
            .pop())
    }
    async fn create_action(&self, action: ActionCreate) -> Result<String, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            id: String,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions")?)
            .json(&action)
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await?
            .id)
    }
    async fn update_action(&self, id: &str, action: ActionUpdate) -> Result<(), Self::Err> {
        self.client
            .put(self.url.join("management/v1/actions/")?.join(id)?)
            .json(&action)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
    async fn delete_action(&self, id: &str) -> Result<(), Self::Err> {
        self.client
            .delete(self.url.join("management/v1/actions/")?.join(id)?)
            .json(&EmptyBody {})
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
    ) -> Result<(), Self::Err> {
        self.client
            .post(
                self.url
                    .join(&format!("management/v1/flows/{flow_type}/trigger/{trigger_type}"))?,
            )
            .json(&serde_json::json!({"actionIds": action_ids}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
