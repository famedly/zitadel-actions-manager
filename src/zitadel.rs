#![allow(async_fn_in_trait)]
// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

//! Interfaces to zitadel.
//!
//! Please ignore traits ending with `Prototype`, they are not meant to be
//! public and are the result of a [`trait_variant::make`] quirks.

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{Action, LoadedScript};
#[cfg(feature = "famedly-zitadel-rust-client")]
use crate::{SpanTraceWrapper, instrument};

/// Supertrait for all handles defined here.
pub trait ZitadelInterface {
    type Err: Send + Sync + std::error::Error + 'static;
}

/// Zitadel handle necessary for [`crate::create_only`].
#[trait_variant::make(ZitadelHandleCreateOnly: Send)]
pub trait ZitadelHandleCreateOnlyPrototype: ZitadelInterface + Sync {
    async fn create_action(
        &self,
        action: ActionCreate,
        org_id: Option<String>,
    ) -> Result<String, Self::Err>;

    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
        org_id: Option<String>,
    ) -> Result<(), Self::Err>;
}

/// Zitadel handle necessary for [`crate::sync`].
#[trait_variant::make(ZitadelHandle: Send)]
pub trait ZitadelHandlePrototype: ZitadelHandleCreateOnly + ZitadelInterface {
    async fn search_actions_by_name(
        &self,
        name: &str,
        org_id: Option<String>,
    ) -> Result<Option<ActionSearch>, Self::Err>;

    async fn update_action(
        &self,
        id: &str,
        action: ActionUpdate,
        org_id: Option<String>,
    ) -> Result<(), Self::Err>;
    async fn delete_action(&self, id: &str, org_id: Option<String>) -> Result<(), Self::Err>;

    async fn get_triggers(
        &self,
        flow_type: &str,
        org_id: Option<String>,
    ) -> Result<Vec<GetTriggersResFlowAction>, Self::Err>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSearch {
    pub id: String,
    pub name: String,
    pub timeout: Option<String>,
    #[serde(default)]
    pub allowed_to_fail: bool,
    pub script: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionCreate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default)]
    pub allowed_to_fail: bool,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionUpdate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default)]
    pub allowed_to_fail: bool,
    pub script: String,
}

impl ActionUpdate {
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

#[must_use]
pub(crate) fn action_is_same(action: &Action<LoadedScript>, a: &ActionSearch) -> bool {
    action.allowed_to_fail == a.allowed_to_fail
        && action.script == a.script
        && (action.timeout == a.timeout
            // 20s is what zitadel apparently sets timeout if we don't set it
            || (action.timeout.is_none() && (a.timeout == Some("20s".into()))))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Id {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTriggersResFlowAction {
    pub trigger_type: Id,
    pub actions: Vec<ActionSearch>,
}

/// Zitadel handle necessary for [`crate::v2::sync`].
#[trait_variant::make(ZitadelHandleV2: Send)]
pub trait ZitadelHandleV2Prototype:
    ZitadelInterface<Err: From<InvalidExpirationDateError>> + Sync
{
    async fn create_target(&self, req: CreateTarget) -> Result<TargetCreated, Self::Err>;
    async fn search_target_by_name(&self, name: &str) -> Result<Option<FoundTarget>, Self::Err>;
    async fn update_target(&self, id: &str, req: UpdateTarget) -> Result<TargetUpdated, Self::Err>;
    async fn delete_target(&self, id: &str) -> Result<(), Self::Err>;

    async fn set_execution(&self, req: Execution) -> Result<(), Self::Err>;
    async fn list_executions(&self) -> Result<Vec<Execution>, Self::Err>;

    async fn list_public_keys(&self, target_id: &str) -> Result<Vec<FoundPublicKey>, Self::Err>;
    async fn add_public_key(
        &self,
        target_id: &str,
        req: AddPublicKey,
    ) -> Result<PublicKeyAdded, Self::Err>;
    async fn activate_public_key(&self, target_id: &str, key_id: &str) -> Result<(), Self::Err>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCreated {
    pub id: String,
    #[serde(default)]
    pub signing_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTarget {
    pub name: String,
    #[serde(flatten)]
    pub target_type: TargetType,
    pub timeout: String,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundTarget {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub target_type: TargetType,
    pub timeout: String,
    pub endpoint: String,
    #[serde(default)]
    pub signing_key: String,
    #[serde(default)]
    pub payload_type: Option<PayloadType>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTarget {
    #[serde(flatten)]
    pub target_type: Option<TargetType>,
    pub timeout: Option<String>,
    pub endpoint: Option<String>,
    pub expiration_signing_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<PayloadType>,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetType {
    restWebhook {
        #[serde(default, rename = "interruptOnError")]
        interrupt_on_error: bool,
    },
    restCall {
        #[serde(default, rename = "interruptOnError")]
        interrupt_on_error: bool,
    },
    restAsync {},
}

/// How Zitadel formats and secures the action payload sent to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum PayloadType {
    #[serde(rename = "PAYLOAD_TYPE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "PAYLOAD_TYPE_JSON")]
    Json,
    #[serde(rename = "PAYLOAD_TYPE_JWT")]
    Jwt,
    #[serde(rename = "PAYLOAD_TYPE_JWE")]
    Jwe,
}

impl PayloadType {
    /// Effective payload type used for sync comparisons (UNSPECIFIED ≡ JSON).
    #[must_use]
    pub fn normalized(self) -> Self {
        match self {
            Self::Unspecified => Self::Json,
            other => other,
        }
    }

    /// Compare optional payload types with Zitadel defaults
    /// (missing/UNSPECIFIED ≡ JSON).
    #[must_use]
    pub fn eq_optional(lhs: Option<Self>, rhs: Option<Self>) -> bool {
        lhs.unwrap_or(Self::Json).normalized() == rhs.unwrap_or(Self::Json).normalized()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetUpdated {
    #[serde(default)]
    pub signing_key: Option<String>,
}

/// Request to upload a public key (PEM) for [`PayloadType::Jwe`] targets.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPublicKey {
    /// PEM-encoded RSA or EC public key (clients base64-encode for the API).
    pub public_key: String,
    pub expiration_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyAdded {
    pub key_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundPublicKey {
    pub key_id: String,
    #[serde(default)]
    pub active: bool,
    /// Base64-encoded PEM as returned by Zitadel.
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub expiration_date: Option<String>,
}

/// Encode a PEM public key as base64 for the Action Service API.
#[must_use]
pub fn encode_public_key_pem(pem: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(pem.as_bytes())
}

/// Decode a base64 public key from the Action Service API into PEM
/// bytes/string.
pub fn decode_public_key_pem(encoded: &str) -> Result<String, base64::DecodeError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Normalize PEM for equality checks (ignore whitespace).
#[must_use]
pub fn normalize_pem(pem: &str) -> String {
    pem.split_whitespace().collect()
}

/// Invalid RFC 3339 public-key expiration date.
#[derive(Debug, Snafu)]
#[snafu(display("Invalid public key expiration date '{value}': expected RFC 3339"))]
pub struct InvalidExpirationDateError {
    /// The unparsable value.
    pub value: String,
}

/// Parse a public-key expiration date as RFC 3339.
pub fn parse_expiration_date(value: &str) -> Result<OffsetDateTime, InvalidExpirationDateError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| InvalidExpirationDateSnafu { value: value.to_owned() }.build())
}

/// Compare optional public-key expiration dates as RFC 3339 instants
/// (so `Z` vs `+00:00` and fractional-second differences still match).
///
/// Returns an error if either present value is not valid RFC 3339.
pub fn expiration_dates_eq(
    lhs: Option<&str>,
    rhs: Option<&str>,
) -> Result<bool, InvalidExpirationDateError> {
    Ok(match (lhs, rhs) {
        (None, None) => true,
        (Some(lhs), Some(rhs)) => parse_expiration_date(lhs)? == parse_expiration_date(rhs)?,
        _ => false,
    })
}

// `serde_yaml` doesn't support nested enums, thus this `singleton_map`
// workaround, see https://github.com/dtolnay/serde-yaml/issues/363#issuecomment-1478409196
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Ord, PartialOrd)]
pub struct Execution {
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub condition: ExecutionCondition,
    pub targets: Vec<String>,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Ord, PartialOrd)]
pub enum ExecutionCondition {
    request(#[serde(with = "serde_yaml::with::singleton_map")] RequestResponseCondition),
    response(#[serde(with = "serde_yaml::with::singleton_map")] RequestResponseCondition),
    function { name: String },
    event(#[serde(with = "serde_yaml::with::singleton_map")] EventCondition),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Ord, PartialOrd)]
pub enum RequestResponseCondition {
    method(String),
    service(String),
    all(TrueConst),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Ord, PartialOrd)]
pub enum EventCondition {
    event(String),
    group(String),
    all(TrueConst),
}

#[test]
fn test_nested_enum_serde_yaml() {
    let execution = Execution {
        targets: vec![],
        condition: ExecutionCondition::event(EventCondition::event("hello".into())),
    };
    let parsed_execution =
        serde_yaml::from_str(&serde_json::to_string(&execution).unwrap()).unwrap();
    assert_eq!(execution, parsed_execution);
}

#[test]
fn target_updated_signing_key_is_optional() {
    let updated: TargetUpdated =
        serde_json::from_str(r#"{"changeDate":"2026-09-29T14:03:54.534194Z"}"#).unwrap();
    assert!(updated.signing_key.is_none());
}

#[test]
fn payload_type_serde_matches_zitadel_wire_names() {
    for (variant, wire) in [
        (PayloadType::Unspecified, "PAYLOAD_TYPE_UNSPECIFIED"),
        (PayloadType::Json, "PAYLOAD_TYPE_JSON"),
        (PayloadType::Jwt, "PAYLOAD_TYPE_JWT"),
        (PayloadType::Jwe, "PAYLOAD_TYPE_JWE"),
    ] {
        assert_eq!(serde_json::to_value(variant).unwrap(), serde_json::json!(wire));
        let parsed: PayloadType = serde_json::from_value(serde_json::json!(wire)).unwrap();
        assert_eq!(parsed, variant);
    }
}

#[test]
fn payload_type_eq_optional_treats_defaults_as_json() {
    assert!(PayloadType::eq_optional(None, Some(PayloadType::Json)));
    assert!(PayloadType::eq_optional(Some(PayloadType::Unspecified), Some(PayloadType::Json)));
    assert!(PayloadType::eq_optional(Some(PayloadType::Jwt), Some(PayloadType::Jwt)));
    assert!(!PayloadType::eq_optional(Some(PayloadType::Jwt), Some(PayloadType::Json)));
}

#[test]
fn expiration_dates_eq_compares_rfc3339_instants() {
    assert!(expiration_dates_eq(None, None).unwrap());
    assert!(!expiration_dates_eq(None, Some("2027-01-01T00:00:00Z")).unwrap());
    assert!(
        expiration_dates_eq(Some("2027-01-01T00:00:00Z"), Some("2027-01-01T00:00:00Z")).unwrap()
    );
    assert!(
        expiration_dates_eq(Some("2027-01-01T00:00:00Z"), Some("2027-01-01T00:00:00.000Z"),)
            .unwrap()
    );
    assert!(
        expiration_dates_eq(Some("2027-01-01T00:00:00Z"), Some("2027-01-01T00:00:00+00:00"),)
            .unwrap()
    );
    assert!(
        !expiration_dates_eq(Some("2027-01-01T00:00:00Z"), Some("2028-01-01T00:00:00Z"),).unwrap()
    );
    assert!(expiration_dates_eq(Some("not-a-date"), Some("not-a-date")).is_err());
    assert!(parse_expiration_date("2027-01-01T00:00:00Z").is_ok());
    assert!(parse_expiration_date("not-a-date").is_err());
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct TrueConst;

impl Serialize for TrueConst {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        true.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TrueConst {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Unexpected;
        (bool::deserialize(deserializer)?)
            .then_some(TrueConst)
            .ok_or_else(|| serde::de::Error::invalid_value(Unexpected::Bool(false), &"true"))
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
use {
    famedly_rust_utils::GenericCombinators,
    famedly_zitadel_rust_client::v2::management::*,
    futures::stream::StreamExt,
    snafu::{GenerateImplicitData, OptionExt as _, ResultExt as _},
};

#[cfg(feature = "famedly-zitadel-rust-client")]
#[derive(Debug, Snafu)]
#[snafu(visibility(pub), context(suffix(false)))]
pub enum FamedlyZrcError {
    #[snafu(display("Zitadel rust client error"))]
    Zrc {
        source: anyhow::Error,
        #[snafu(implicit)]
        context: SpanTraceWrapper,
    },
    #[snafu(display("Missing field '{field}' in zitadel response"))]
    MissingField {
        field: String,
        #[snafu(implicit)]
        context: SpanTraceWrapper,
    },
    #[snafu(display(
        "famedly_zitadel_rust_client backend doesn't support non-numeric flow and trigger types"
    ))]
    FlowTypeParse {
        source: std::num::ParseIntError,
        #[snafu(implicit)]
        context: SpanTraceWrapper,
    },
    #[snafu(display("Malformed response from zitadel. Malformed object: {object}"))]
    MalformedResponse {
        object: &'static str,
        #[snafu(implicit)]
        context: SpanTraceWrapper,
    },
    #[snafu(display("{message}"))]
    InvalidExpirationDate {
        message: String,
        #[snafu(implicit)]
        context: SpanTraceWrapper,
    },
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl From<InvalidExpirationDateError> for FamedlyZrcError {
    fn from(error: InvalidExpirationDateError) -> Self {
        InvalidExpirationDate { message: error.to_string() }.build()
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl ZitadelInterface for famedly_zitadel_rust_client::v2::Zitadel {
    type Err = FamedlyZrcError;
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl ZitadelHandleCreateOnly for famedly_zitadel_rust_client::v2::Zitadel {
    #[instrument(skip(self))]
    async fn create_action(
        &self,
        action: ActionCreate,
        org_id: Option<String>,
    ) -> Result<String, Self::Err> {
        self.create_action(action.into(), org_id)
            .await
            .context(Zrc)?
            .id()
            .cloned()
            .context(MissingField { field: "id" })
    }

    #[instrument(skip(self))]
    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        let flow_type = flow_type.parse::<u32>().context(FlowTypeParse)?;
        let trigger_type = trigger_type.parse::<u32>().context(FlowTypeParse)?;
        self.set_trigger_actions(
            flow_type,
            trigger_type,
            ManagementServiceSetTriggerActionsBody::new().with_action_ids(action_ids),
            org_id,
        )
        .await
        .context(Zrc)?;
        Ok(())
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl ZitadelHandle for famedly_zitadel_rust_client::v2::Zitadel {
    #[instrument(skip(self))]
    async fn search_actions_by_name(
        &self,
        name: &str,
        org_id: Option<String>,
    ) -> Result<Option<ActionSearch>, Self::Err> {
        // TODO: explicitly set TEXT_QUERY_METHOD_EQUALS

        self.list_actions(
            org_id,
            None,
            Some(vec![
                V1ActionQuery::new()
                    .with_action_name_query(V1ActionNameQuery::new().with_name(name.into())),
            ]),
        )
        .context(Zrc)?
        .next()
        .await
        .transpose()
        .context(Zrc)?
        .map(TryInto::try_into)
        .transpose()
    }

    #[instrument(skip(self))]
    async fn update_action(
        &self,
        id: &str,
        action: ActionUpdate,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        self.update_action(id.into(), action.into(), org_id).await.context(Zrc)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete_action(&self, id: &str, org_id: Option<String>) -> Result<(), Self::Err> {
        self.delete_action(id.into(), org_id).await.context(Zrc)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_triggers(
        &self,
        flow_type: &str,
        org_id: Option<String>,
    ) -> Result<Vec<GetTriggersResFlowAction>, Self::Err> {
        let flow_type = flow_type.parse::<u32>().context(FlowTypeParse)?;
        from_flow_response(self.get_flow(flow_type, org_id).await.context(Zrc)?)
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl TryFrom<V1Action> for ActionSearch {
    type Error = FamedlyZrcError;
    fn try_from(a: V1Action) -> Result<ActionSearch, Self::Error> {
        Ok(ActionSearch {
            id: a.id().context(MissingField { field: "id" })?.into(),
            name: a.name().context(MissingField { field: "name" })?.into(),
            timeout: a.timeout().cloned(),
            allowed_to_fail: a.allowed_to_fail().copied().unwrap_or_default(),
            script: a.script().context(MissingField { field: "script" })?.into(),
        })
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl From<ActionCreate> for V1CreateActionRequest {
    fn from(a: ActionCreate) -> Self {
        Self::new(a.name, a.script)
            .chain_opt(a.timeout, Self::with_timeout)
            .with_allowed_to_fail(a.allowed_to_fail)
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl From<ActionUpdate> for ManagementServiceUpdateActionBody {
    fn from(a: ActionUpdate) -> Self {
        Self::new(a.name, a.script)
            .chain_opt(a.timeout, Self::with_timeout)
            .with_allowed_to_fail(a.allowed_to_fail)
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
fn from_flow_response(
    a: V1GetFlowResponse,
) -> Result<Vec<GetTriggersResFlowAction>, FamedlyZrcError> {
    let flow = a.flow().context(MissingField { field: "flow" })?;
    flow.trigger_actions().map_or_else(
        || Ok(Vec::new()),
        |trigger_actions| {
            trigger_actions
                .iter()
                .map(|trigger_action| {
                    Ok(GetTriggersResFlowAction {
                        trigger_type: Id {
                            id: trigger_action
                                .trigger_type()
                                .context(MissingField { field: "trigger_type" })?
                                .id()
                                .context(MissingField { field: "trigger_type.id" })?
                                .into(),
                        },
                        actions: trigger_action.actions().map_or_else(
                            || Ok(Vec::new()),
                            |actions| {
                                actions
                                    .iter()
                                    .cloned()
                                    .map(TryInto::try_into)
                                    .collect::<Result<Vec<_>, _>>()
                            },
                        )?,
                    })
                })
                .collect::<Result<_, FamedlyZrcError>>()
        },
    )
}

#[cfg(feature = "famedly-zitadel-rust-client")]
use {
    famedly_zitadel_rust_client::v2::actions::*,
    famedly_zitadel_rust_client::v2::pagination::PaginationParams, futures::stream::TryStreamExt,
};

#[cfg(feature = "famedly-zitadel-rust-client")]
impl ZitadelHandleV2 for famedly_zitadel_rust_client::v2::Zitadel {
    #[instrument(skip_all, fields(name = req_.name))]
    async fn create_target(&self, req_: CreateTarget) -> Result<TargetCreated, Self::Err> {
        let mut req =
            V2CreateTargetRequest::new(req_.name, req_.endpoint).with_timeout(req_.timeout);
        if let Some(payload_type) = req_.payload_type {
            req = req.with_payload_type(payload_type.into());
        }
        match req_.target_type {
            TargetType::restWebhook { interrupt_on_error } => req
                .set_rest_webhook(V2RestWebhook::new().with_interrupt_on_error(interrupt_on_error)),
            TargetType::restCall { interrupt_on_error } => {
                req.set_rest_call(V2RestCall::new().with_interrupt_on_error(interrupt_on_error));
            }

            TargetType::restAsync {} => req.set_rest_async(V2RestAsync::new()),
        }

        let res = self.create_target(&req).await.context(Zrc)?;
        Ok(TargetCreated {
            id: res.id().cloned().context(MissingField { field: "id" })?,
            signing_key: res.signing_key().cloned().unwrap_or_default(),
        })
    }

    #[instrument(skip_all, fields(id))]
    async fn update_target(
        &self,
        id: &str,
        req_: UpdateTarget,
    ) -> Result<TargetUpdated, Self::Err> {
        type Req = ActionServiceUpdateTargetBody;
        let mut req = Req::new()
            .chain_opt(req_.timeout, Req::with_timeout)
            .chain_opt(req_.endpoint, Req::with_endpoint)
            .chain_opt(req_.expiration_signing_key, Req::with_expiration_signing_key)
            .chain_opt(req_.payload_type.map(Into::into), Req::with_payload_type);
        match req_.target_type {
            Some(TargetType::restWebhook { interrupt_on_error }) => req
                .set_rest_webhook(V2RestWebhook::new().with_interrupt_on_error(interrupt_on_error)),
            Some(TargetType::restCall { interrupt_on_error }) => {
                req.set_rest_call(V2RestCall::new().with_interrupt_on_error(interrupt_on_error));
            }
            Some(TargetType::restAsync {}) => req.set_rest_async(V2RestAsync::new()),
            None => {}
        }
        let res = self.update_target(id, &req).await.context(Zrc)?;
        Ok(TargetUpdated { signing_key: res.signing_key().cloned() })
    }

    #[instrument(skip(self))]
    async fn delete_target(&self, id: &str) -> Result<(), Self::Err> {
        self.delete_target(id).await.context(Zrc)?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn search_target_by_name(&self, name: &str) -> Result<Option<FoundTarget>, Self::Err> {
        let target = std::pin::pin!(self.list_targets(
            &Some(PaginationParams::default().with_page_size(1)),
            &None,
            &Some(vec![V2TargetSearchFilter::new().with_target_name_filter(
                V2TargetNameFilter::new()
                    .with_target_name(name.to_owned())
                    .with_method(V2TextFilterMethod::TEXT_FILTER_METHOD_EQUALS),
            )]),
        ))
        .next()
        .await
        .transpose()
        .context(Zrc)?;

        let Some(target) = target else {
            return Ok(None);
        };

        let target_type = if let Some(x) = target.rest_webhook() {
            TargetType::restWebhook {
                interrupt_on_error: x.interrupt_on_error().copied().unwrap_or_default(),
            }
        } else if let Some(x) = target.rest_call() {
            TargetType::restCall {
                interrupt_on_error: x.interrupt_on_error().copied().unwrap_or_default(),
            }
        } else if target.rest_async().is_some() {
            TargetType::restAsync {}
        } else {
            return MalformedResponse { object: "Found target" }.fail();
        };

        Ok(Some(FoundTarget {
            id: target.id().cloned().context(MissingField { field: "id" })?,
            name: target.name().cloned().context(MissingField { field: "name" })?,
            target_type,
            timeout: target.timeout().cloned().context(MissingField { field: "timeout" })?,
            endpoint: target.endpoint().cloned().context(MissingField { field: "endpoint" })?,
            signing_key: target.signing_key().cloned().unwrap_or_default(),
            payload_type: target.payload_type().cloned().map(Into::into),
        }))
    }

    // Writing these two last methods manually was soul crushing.
    // Please use AI next time, spare yourself.
    #[instrument(skip_all)]
    async fn set_execution(&self, req: Execution) -> Result<(), Self::Err> {
        let condition = match req.condition {
            ExecutionCondition::request(cnd) => V2Condition::new().with_request(match cnd {
                RequestResponseCondition::method(x) => V2RequestExecution::new().with_method(x),
                RequestResponseCondition::service(x) => V2RequestExecution::new().with_service(x),
                RequestResponseCondition::all(_) => V2RequestExecution::new().with_all(true),
            }),
            ExecutionCondition::response(cnd) => V2Condition::new().with_response(match cnd {
                RequestResponseCondition::method(x) => V2ResponseExecution::new().with_method(x),
                RequestResponseCondition::service(x) => V2ResponseExecution::new().with_service(x),
                RequestResponseCondition::all(_) => V2ResponseExecution::new().with_all(true),
            }),
            ExecutionCondition::function { name } => {
                V2Condition::new().with_function(V2FunctionExecution::new().with_name(name))
            }
            ExecutionCondition::event(cnd) => V2Condition::new().with_event(match cnd {
                EventCondition::event(x) => V2EventExecution::new().with_event(x),
                EventCondition::group(x) => V2EventExecution::new().with_group(x),
                EventCondition::all(_) => V2EventExecution::new().with_all(true),
            }),
        };
        self.set_execution(
            &V2SetExecutionRequest::new().with_condition(condition).with_targets(req.targets),
        )
        .await
        .context(Zrc)?;
        Ok(())
    }

    #[instrument(skip_all)]
    async fn list_executions(&self) -> Result<Vec<Execution>, Self::Err> {
        self.list_executions(&None, &None, &None)
            .map_err(|e| FamedlyZrcError::Zrc { source: e, context: SpanTraceWrapper::generate() })
            .and_then(async |execution| {
                let condition =
                    execution.condition().context(MissingField { field: "condition" })?;
                let condition = if let Some(request) = condition.request() {
                    ExecutionCondition::request(if let Some(method) = request.method() {
                        RequestResponseCondition::method(method.to_owned())
                    } else if let Some(service) = request.service() {
                        RequestResponseCondition::service(service.to_owned())
                    } else if let Some(_all) = request.all() {
                        RequestResponseCondition::all(TrueConst)
                    } else {
                        return MalformedResponse { object: "Execution.condition.request" }.fail();
                    })
                } else if let Some(response) = condition.response() {
                    ExecutionCondition::response(if let Some(method) = response.method() {
                        RequestResponseCondition::method(method.to_owned())
                    } else if let Some(service) = response.service() {
                        RequestResponseCondition::service(service.to_owned())
                    } else if let Some(_all) = response.all() {
                        RequestResponseCondition::all(TrueConst)
                    } else {
                        return MalformedResponse { object: "Execution.condition.response" }.fail();
                    })
                } else if let Some(function) = condition.function() {
                    ExecutionCondition::function {
                        name: function.name().context(MissingField { field: "name" })?.to_owned(),
                    }
                } else if let Some(event) = condition.event() {
                    ExecutionCondition::event(if let Some(event) = event.event() {
                        EventCondition::event(event.to_owned())
                    } else if let Some(group) = event.group() {
                        EventCondition::group(group.to_owned())
                    } else if event.all().is_some() {
                        EventCondition::all(TrueConst)
                    } else {
                        return MalformedResponse { object: "Execution.condition.event" }.fail();
                    })
                } else {
                    return MalformedResponse { object: "Execution.condition" }.fail();
                };
                Ok(Execution {
                    condition,
                    targets: execution.targets().cloned().unwrap_or_default(),
                })
            })
            .try_collect()
            .await
    }

    #[instrument(skip(self))]
    async fn list_public_keys(&self, target_id: &str) -> Result<Vec<FoundPublicKey>, Self::Err> {
        let res = self
            .list_public_keys(
                target_id,
                &ActionServiceListPublicKeysBody::new()
                    .with_pagination(V2PaginationRequest::new().with_limit(1000)),
            )
            .await
            .context(Zrc)?;
        Ok(res
            .public_keys()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|key| {
                Some(FoundPublicKey {
                    key_id: key.key_id().cloned()?,
                    active: key.active().copied().unwrap_or(false),
                    public_key: key.public_key().cloned(),
                    fingerprint: key.fingerprint().cloned(),
                    expiration_date: key.expiration_date().cloned(),
                })
            })
            .collect())
    }

    #[instrument(skip_all, fields(target_id))]
    async fn add_public_key(
        &self,
        target_id: &str,
        req: AddPublicKey,
    ) -> Result<PublicKeyAdded, Self::Err> {
        let mut body = ActionServiceAddPublicKeyBody::new(encode_public_key_pem(&req.public_key));
        if let Some(expiration) = req.expiration_date {
            body = body.with_expiration_date(expiration);
        }
        let res = self.add_public_key(target_id, &body).await.context(Zrc)?;
        Ok(PublicKeyAdded {
            key_id: res.key_id().cloned().context(MissingField { field: "key_id" })?,
        })
    }

    #[instrument(skip(self))]
    async fn activate_public_key(&self, target_id: &str, key_id: &str) -> Result<(), Self::Err> {
        self.activate_public_key(target_id, key_id).await.context(Zrc)?;
        Ok(())
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl From<PayloadType> for V2PayloadType {
    fn from(value: PayloadType) -> Self {
        match value {
            PayloadType::Unspecified => Self::PAYLOAD_TYPE_UNSPECIFIED,
            PayloadType::Json => Self::PAYLOAD_TYPE_JSON,
            PayloadType::Jwt => Self::PAYLOAD_TYPE_JWT,
            PayloadType::Jwe => Self::PAYLOAD_TYPE_JWE,
        }
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl From<V2PayloadType> for PayloadType {
    fn from(value: V2PayloadType) -> Self {
        match value {
            V2PayloadType::PAYLOAD_TYPE_UNSPECIFIED => Self::Unspecified,
            V2PayloadType::PAYLOAD_TYPE_JSON => Self::Json,
            V2PayloadType::PAYLOAD_TYPE_JWT => Self::Jwt,
            V2PayloadType::PAYLOAD_TYPE_JWE => Self::Jwe,
        }
    }
}
