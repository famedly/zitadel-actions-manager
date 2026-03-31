// SPDX-FileCopyrightText: 2026 Famedly GmbH
//
// SPDX-License-Identifier: Apache-2.0

//! [`reqwest`]-based simple client, with no reauth functionality. Used by the
//! CLI tool.

use famedly_rust_utils::{BaseUrl, GenericCombinators, reqwest::*};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::{SpanTraceWrapper, instrument, zitadel::*};

/// Header for Zitadel organization ID
const HEADER_ZITADEL_ORGANIZATION_ID: &str = "x-zitadel-orgid";

/// Simple client that requires access token. This client does not do oauth and
/// token renewal. Used by the CLI tool.
#[derive(Debug, Clone)]
pub struct SimpleZitadelClient {
	client: reqwest::Client,
	url: BaseUrl,
}

use reqwest::header::{AUTHORIZATION, HeaderMap};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub), context(suffix(false)))]
pub enum SimpleZitadelClientCreationError {
	#[snafu(display("http request failed"))]
	Reqwest {
		source: reqwest::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("header parsing failed"))]
	HeaderParsing {
		source: reqwest::header::InvalidHeaderValue,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
}

impl SimpleZitadelClientCreationError {
	#[must_use]
	pub fn get_context(&self) -> &SpanTraceWrapper {
		match self {
			Self::Reqwest { context, .. } => context,
			Self::HeaderParsing { context, .. } => context,
		}
	}
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
					headers.insert(
						AUTHORIZATION,
						format!("Bearer {token}").parse().context(HeaderParsing)?,
					);
					if let Some(org_id) = org_id {
						headers.insert("x-zitadel-orgid", org_id.parse().context(HeaderParsing)?);
					}
					headers
				})
				.build()
				.context(Reqwest)?,
			url,
		})
	}
	#[doc(hidden)]
	/// Create an organization. Used in tests.
	pub async fn create_org(&self, org_name: &str) -> Result<String, SimpleZitadelClientError> {
		#[derive(Deserialize)]
		#[serde(rename_all = "camelCase")]
		struct Response {
			organization_id: String,
		}

		Ok(self
			.client
			.post(self.url.join("v2/organizations").context(Url)?)
			.json(&serde_json::json!({ "name": org_name }))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
			.organization_id)
	}
	#[doc(hidden)]
	/// List targets id. Used in tests.
	pub async fn list_targets_id(&self) -> Result<Vec<String>, SimpleZitadelClientError> {
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
			.post(self.url.join("v2beta/actions/targets/search").context(Url)?)
			.json(&serde_json::json!({
				"pagination": { "limit": 1000 },
				"filters": []
			}))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
			.targets
			.into_iter()
			.map(|target| target.id)
			.collect())
	}
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub), context(suffix(false)))]
pub enum SimpleZitadelClientError {
	#[snafu(display("serde failed"))]
	Serde {
		source: reqwest::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("http transport failure"))]
	ReqwestTransport {
		source: reqwest::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("http request failed"))]
	ReqwestService {
		source: ReqwestErrorWithBody,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("url parsing failed"))]
	Url {
		source: url::ParseError,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("jwt error"))]
	JWT {
		source: jsonwebtoken::errors::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
}

impl SimpleZitadelClientError {
	#[must_use]
	pub fn get_context(&self) -> &SpanTraceWrapper {
		match self {
			Self::Serde { context, .. } => context,
			Self::ReqwestTransport { context, .. } => context,
			Self::ReqwestService { context, .. } => context,
			Self::Url { context, .. } => context,
			Self::JWT { context, .. } => context,
		}
	}
}

#[derive(Serialize)]
struct EmptyBody {}

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
	type Err = SimpleZitadelClientError;
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
			.post(self.url.join("management/v1/actions").context(Url)?)
			.chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
			.json(&action)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
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
					.context(Url)?,
			)
			.chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
			.json(&serde_json::json!({"actionIds": action_ids}))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?;
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
			.post(self.url.join("management/v1/actions/_search").context(Url)?)
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
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
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
			.put(self.url.join("management/v1/actions/").and_then(|u| u.join(id)).context(Url)?)
			.chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
			.json(&action)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?;
		Ok(())
	}

	#[instrument(skip(self))]
	async fn delete_action(&self, id: &str, org_id: Option<String>) -> Result<(), Self::Err> {
		self.client
			.delete(self.url.join("management/v1/actions/").and_then(|u| u.join(id)).context(Url)?)
			.chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
			.json(&EmptyBody {})
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?;
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
					.and_then(|u| u.join(flow_type))
					.context(Url)?,
			)
			.chain_opt(org_id, |req, org_id| req.header(HEADER_ZITADEL_ORGANIZATION_ID, org_id))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<GetTriggersRes>()
			.await
			.context(Serde)?
			.flow
			.trigger_actions)
	}
}

impl ZitadelHandleV2 for SimpleZitadelClient {
	#[instrument(skip(self))]
	async fn create_target(&self, req: CreateTarget) -> Result<TargetCreated, Self::Err> {
		self.client
			.post(self.url.join("v2beta/actions/targets").context(Url)?)
			.json(&req)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<TargetCreated>()
			.await
			.context(Serde)
	}

	#[instrument(skip(self))]
	async fn search_target_by_name(&self, name: &str) -> Result<Option<FoundTarget>, Self::Err> {
		#[derive(Deserialize)]
		struct Response {
			targets: Option<Vec<FoundTarget>>,
		}
		Ok(self
			.client
			.post(self.url.join("v2beta/actions/targets/search").context(Url)?)
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
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
			.targets
			.and_then(|mut result| result.pop()))
	}

	#[instrument(skip(self))]
	async fn update_target(&self, id: &str, req: UpdateTarget) -> Result<TargetUpdated, Self::Err> {
		self.client
			.post(self.url.join("v2beta/actions/targets/").and_then(|u| u.join(id)).context(Url)?)
			.json(&req)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<TargetUpdated>()
			.await
			.context(Serde)
	}

	#[instrument(skip(self))]
	async fn delete_target(&self, id: &str) -> Result<(), Self::Err> {
		self.client
			.delete(self.url.join("v2beta/actions/targets/").and_then(|u| u.join(id)).context(Url)?)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?;
		Ok(())
	}

	#[instrument(skip(self))]
	async fn set_execution(&self, req: Execution) -> Result<(), Self::Err> {
		self.client
			.put(self.url.join("v2beta/actions/executions").context(Url)?)
			.json(&req)
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?;
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
			.post(self.url.join("v2beta/actions/executions/search").context(Url)?)
			.json(&serde_json::json!({
				"pagination": { "limit": 1000 }
			}))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
			.executions
			.unwrap_or_default())
	}
}

/// Zitadel service account
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
	key_id: String,
	key: String,
	user_id: String,
}

/// Authenticates to zitadel given a service account returning an access token.
#[instrument(skip(sa, url), fields(%url))]
pub async fn auth_with_service_account(
	url: &BaseUrl,
	aud: &str,
	sa: &ServiceAccount,
) -> Result<String, SimpleZitadelClientError> {
	use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};

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
		&EncodingKey::from_rsa_pem(sa.key.as_bytes()).context(JWT)?,
	)
	.context(JWT)?;

	let client = reqwest::Client::builder()
		.timeout(std::time::Duration::from_secs(1))
		.build()
		.context(ReqwestTransport)?;

	Ok(client
		.post(url.join("oauth/v2/token").context(Url)?)
		.form(&[
			("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
			("scope", "openid urn:zitadel:iam:org:project:id:zitadel:aud"),
			("assertion", &assertion),
		])
		.send()
		.await
		.context(ReqwestTransport)?
		.error_for_status_with_body()
		.await
		.context(ReqwestService)?
		.json::<Response>()
		.await
		.context(Serde)?
		.access_token)
}

impl SimpleZitadelClient {
	#[instrument(skip(self))]
	pub async fn get_all_orgs(
		&self,
		offset: u64,
		limit: u64,
	) -> Result<Option<Vec<String>>, SimpleZitadelClientError> {
		#[derive(Deserialize)]
		struct Response {
			result: Option<Vec<Id>>,
		}
		Ok(self
			.client
			.post(self.url.join("/v2/organizations/_search").context(Url)?)
			.json(&serde_json::json!({"query": {
			  "offset": offset,
			  "limit": limit,
			}}))
			.send()
			.await
			.context(ReqwestTransport)?
			.error_for_status_with_body()
			.await
			.context(ReqwestService)?
			.json::<Response>()
			.await
			.context(Serde)?
			.result
			.map(|result| result.into_iter().map(|id| id.id).collect()))
	}
}
