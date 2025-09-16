use famedly_rust_utils::{reqwest::*, BaseUrl, GenericCombinators};
use serde::{Deserialize, Serialize};

use crate::{instrument, zitadel::*};

/// Header for Zitadel organization ID
const HEADER_ZITADEL_ORGANIZATION_ID: &str = "x-zitadel-orgid";

/// Simple client that requires access token. This client does not do oauth and
/// token renewal. Used by the cli tool.
#[derive(Debug, Clone)]
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
        token: &str,
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
    #[doc(hidden)]
    /// Create an organization. Used in tests.
    pub async fn create_org(&self, org_name: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            organization_id: String,
        }

        Ok(self
            .client
            .post(self.url.join("v2/organizations").map_err(E::from)?)
            .json(&serde_json::json!({ "name": org_name }))
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<Response>()
            .await
            .map_err(E::from)?
            .organization_id)
    }
    #[doc(hidden)]
    /// List targets id. Used in tests.
    pub async fn list_targets_id(&self) -> anyhow::Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            targets: Vec<Target>,
        }
        #[derive(Deserialize)]
        struct Target {
            id: String,
        }
        Ok(self
            .client
            .post(self.url.join("v2beta/actions/targets/search").map_err(E::from)?)
            .json(&serde_json::json!({
                "pagination": { "limit": 1000 },
                "filters": []
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
            .targets
            .into_iter()
            .map(|target| target.id)
            .collect())
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
    #[error("jwt error: {0}")]
    JWT(#[from] jsonwebtoken::errors::Error),
}

#[derive(Serialize)]
struct EmptyBody {}

use crate::Traced;

crate::impl_traced_from!(SimpleZitadelClientError);

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

impl ZitadelInterface for SimpleZitadelClient {
    type Err = Traced<SimpleZitadelClientError>;
}

impl ZitadelHandleCreateOnly for SimpleZitadelClient {
    #[instrument(skip(self))]
    async fn create_action(
        &self,
        action: ActionCreate,
        org_id: Option<String>,
    ) -> Result<String, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            id: String,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions").map_err(E::from)?)
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
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

    #[instrument(skip(self))]
    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        self.client
            .post(
                self.url
                    .join(&format!("management/v1/flows/{flow_type}/trigger/{trigger_type}"))
                    .map_err(E::from)?,
            )
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
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

impl ZitadelHandle for SimpleZitadelClient {
    #[instrument(skip(self))]
    async fn search_actions_by_name(
        &self,
        name: &str,
        org_id: Option<String>,
    ) -> Result<Option<ActionSearch>, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            result: Option<Vec<ActionSearch>>,
        }
        Ok(self
            .client
            .post(self.url.join("management/v1/actions/_search").map_err(E::from)?)
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
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

    #[instrument(skip(self))]
    async fn update_action(
        &self,
        id: &str,
        action: ActionUpdate,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        self.client
            .put(
                self.url
                    .join("management/v1/actions/")
                    .map_err(E::from)?
                    .join(id)
                    .map_err(E::from)?,
            )
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
            .json(&action)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete_action(&self, id: &str, org_id: Option<String>) -> Result<(), Self::Err> {
        self.client
            .delete(
                self.url
                    .join("management/v1/actions/")
                    .map_err(E::from)?
                    .join(id)
                    .map_err(E::from)?,
            )
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
            .json(&EmptyBody {})
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_triggers(
        &self,
        flow_type: &str,
        org_id: Option<String>,
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
            .chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
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
}

impl ZitadelHandleV2 for SimpleZitadelClient {
    #[instrument(skip(self))]
    async fn create_target(&self, req: CreateTarget) -> Result<TargetCreated, Self::Err> {
        Ok(self
            .client
            .post(self.url.join("v2beta/actions/targets").map_err(E::from)?)
            .json(&req)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<TargetCreated>()
            .await
            .map_err(E::from)?)
    }

    #[instrument(skip(self))]
    async fn search_target_by_name(&self, name: &str) -> Result<Option<FoundTarget>, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            targets: Option<Vec<FoundTarget>>,
        }
        Ok(self
            .client
            .post(self.url.join("v2beta/actions/targets/search").map_err(E::from)?)
            .json(&serde_json::json!({
                "pagination": { "limit": 1 },
                "filters": [{
                    "targetNameFilter": {
                        "targetName": name,
                        "method": "TEXT_FILTER_METHOD_EQUALS",
                    }
                }]
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
            .targets
            .and_then(|mut result| result.pop()))
    }

    #[instrument(skip(self))]
    async fn update_target(&self, id: &str, req: UpdateTarget) -> Result<TargetUpdated, Self::Err> {
        Ok(self
            .client
            .post(
                self.url
                    .join("v2beta/actions/targets/")
                    .and_then(|u| u.join(id))
                    .map_err(E::from)?,
            )
            .json(&req)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?
            .json::<TargetUpdated>()
            .await
            .map_err(E::from)?)
    }

    #[instrument(skip(self))]
    async fn delete_target(&self, id: &str) -> Result<(), Self::Err> {
        self.client
            .delete(
                self.url
                    .join("v2beta/actions/targets/")
                    .and_then(|u| u.join(id))
                    .map_err(E::from)?,
            )
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn set_execution(&self, req: Execution) -> Result<(), Self::Err> {
        self.client
            .put(self.url.join("v2beta/actions/executions").map_err(E::from)?)
            .json(&req)
            .send()
            .await
            .map_err(E::from)?
            .error_for_status_with_body()
            .await
            .map_err(E::from)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn list_executions(&self) -> Result<Vec<Execution>, Self::Err> {
        #[derive(Deserialize)]
        struct Response {
            executions: Option<Vec<Execution>>,
        }
        Ok(self
            .client
            .post(self.url.join("v2beta/actions/executions/search").map_err(E::from)?)
            .json(&serde_json::json!({
                "pagination": { "limit": 1000 }
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
            .executions
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    key_id: String,
    key: String,
    user_id: String,
}

#[instrument(skip(sa, url), fields(%url))]
pub async fn auth_with_service_account(
    url: &BaseUrl,
    aud: &str,
    sa: &ServiceAccount,
) -> Result<String, Traced<SimpleZitadelClientError>> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    #[derive(Debug, Clone, Deserialize)]
    struct Response {
        access_token: String,
    }

    let now = time::OffsetDateTime::now_utc();
    let assertion = encode(
        &Header::new(Algorithm::RS256).mutate(|h| h.kid = Some(sa.key_id.clone())),
        &serde_json::json!({
            "aud": [aud],
            "sub": sa.user_id,
            "iss": sa.user_id,
            "exp": (now + std::time::Duration::from_secs(60)).unix_timestamp(),
            "iat": now.unix_timestamp(),
        }),
        &EncodingKey::from_rsa_pem(sa.key.as_bytes()).map_err(E::from)?,
    )
    .map_err(E::from)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .map_err(E::from)?;

    Ok(client
        .post(url.join("oauth/v2/token").map_err(E::from)?)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("scope", "openid urn:zitadel:iam:org:project:id:zitadel:aud"),
            ("assertion", &assertion),
        ])
        .send()
        .await
        .map_err(E::from)?
        .error_for_status_with_body()
        .await
        .map_err(E::from)?
        .json::<Response>()
        .await
        .map_err(E::from)?
        .access_token)
}

impl SimpleZitadelClient {
    #[instrument(skip(self))]
    pub async fn get_all_orgs(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<Option<Vec<String>>, Traced<SimpleZitadelClientError>> {
        #[derive(Deserialize)]
        struct Response {
            result: Option<Vec<Id>>,
        }
        Ok(self
            .client
            .post(self.url.join("/v2/organizations/_search").map_err(E::from)?)
            .json(&serde_json::json!({"query": {
              "offset": offset,
              "limit": limit,
            }}))
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
            .map(|result| result.into_iter().map(|id| id.id).collect()))
    }
}
