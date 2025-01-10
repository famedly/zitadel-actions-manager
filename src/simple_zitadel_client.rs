use famedly_rust_utils::{reqwest::*, BaseUrl};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::zitadel::*;

/// Simple client that requires access token. This client does not do oauth and
/// token renewal. Used by the cli tool.
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
    #[error("http transport failure: {0}")]
    ReqwestTransport(#[from] reqwest::Error),
    #[error("http request failed: {0}")]
    ReqwestService(#[from] ReqwestErrorWithBody),
    #[error("url parsing failed: {0}")]
    Url(#[from] url::ParseError),
}

#[derive(Serialize)]
struct EmptyBody {}

use crate::Traced;

impl From<SimpleZitadelClientError> for Traced<SimpleZitadelClientError> {
    fn from(error: SimpleZitadelClientError) -> Self {
        Self { error, span: tracing_error::SpanTrace::capture() }
    }
}

/// Short alias to do `.map_err(E::from)?`
type E = SimpleZitadelClientError;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GetTriggersRes {
    flow: GetTriggersResFlow,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GetTriggersResFlow {
    r#type: Id,
    // state: FLOW_STATE_ACTIVE
    #[serde(default = "Vec::new")]
    trigger_actions: Vec<GetTriggersResFlowAction>,
}

impl ZitadelHandle for SimpleZitadelClient {
    type Err = Traced<SimpleZitadelClientError>;

    #[instrument(skip(self), level = "error")]
    async fn search_actions_by_name(&self, name: &str) -> Result<Option<ActionSearch>, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            result: Option<Vec<ActionSearch>>,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions/_search").map_err(E::from)?)
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
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<Response>()
            .await
            .map_err(E::from)?
            .result
            .and_then(|mut result| result.pop()))
    }

    #[instrument(skip(self), level = "error")]
    async fn create_action(&self, action: ActionCreate) -> Result<String, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            id: String,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions").map_err(E::from)?)
            .json(&action)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<Response>()
            .await
            .map_err(E::from)?
            .id)
    }

    #[instrument(skip(self), level = "error")]
    async fn update_action(&self, id: &str, action: ActionUpdate) -> Result<(), Self::Err> {
        self.client
            .put(
                self.url
                    .join("management/v1/actions/")
                    .map_err(E::from)?
                    .join(id)
                    .map_err(E::from)?,
            )
            .json(&action)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self), level = "error")]
    async fn delete_action(&self, id: &str) -> Result<(), Self::Err> {
        self.client
            .delete(
                self.url
                    .join("management/v1/actions/")
                    .map_err(E::from)?
                    .join(id)
                    .map_err(E::from)?,
            )
            .json(&EmptyBody {})
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self), level = "error")]
    async fn get_triggers(
        &self,
        flow_type: &str,
    ) -> Result<Vec<GetTriggersResFlowAction>, Self::Err> {
        Ok(self
            .client
            .get(
                self.url
                    .join("management/v1/flows/")
                    .map_err(E::from)?
                    .join(flow_type)
                    .map_err(E::from)?,
            )
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<GetTriggersRes>()
            .await
            .map_err(E::from)?
            .flow
            .trigger_actions)
    }

    #[instrument(skip(self), level = "error")]
    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
    ) -> Result<(), Self::Err> {
        self.client
            .post(
                self.url
                    .join(&format!("management/v1/flows/{flow_type}/trigger/{trigger_type}"))
                    .map_err(E::from)?,
            )
            .json(&serde_json::json!({"actionIds": action_ids}))
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }
}
