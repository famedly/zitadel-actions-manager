#![allow(async_fn_in_trait)]

use serde::{Deserialize, Serialize};

use crate::{Action, LoadedScript};

pub trait ZitadelInterface {
    type Err: Send + Sync;
}

#[trait_variant::make(ZitadelHandleCreateOnly: Send + Sync)]
pub trait ZitadelHandleCreateOnlyPrototype: ZitadelInterface {
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

#[trait_variant::make(ZitadelHandle: Send + Sync)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionUpdate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_to_fail: Option<bool>,
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
pub fn action_is_same(action: &Action<LoadedScript>, a: &ActionSearch) -> bool {
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

#[trait_variant::make(ZitadelHandleV2: Send + Sync)]
pub trait ZitadelHandleV2Prototype: ZitadelInterface {
    async fn create_target(&self, req: CreateTarget) -> Result<TargetCreated, Self::Err>;
    async fn search_target_by_name(&self, name: &str) -> Result<Option<FoundTarget>, Self::Err>;
    async fn update_target(&self, id: &str, req: UpdateTarget) -> Result<TargetUpdated, Self::Err>;
    async fn delete_target(&self, id: &str) -> Result<(), Self::Err>;

    async fn set_execution(&self, req: Execution) -> Result<(), Self::Err>;
    async fn list_executions(&self) -> Result<Vec<Execution>, Self::Err>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCreated {
    pub id: String,
    pub signing_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTarget {
    pub name: String,
    #[serde(flatten)]
    pub target_type: TargetType,
    pub timeout: String,
    pub endpoint: url::Url,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundTarget {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub target_type: TargetType,
    pub timeout: String,
    pub endpoint: url::Url,
    pub signing_key: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTarget {
    #[serde(flatten)]
    pub target_type: Option<TargetType>,
    pub timeout: Option<String>,
    pub endpoint: Option<url::Url>,
    pub expiration_signing_key: Option<String>,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum TargetType {
    restWebhook(Asdf),
    restCall(Asdf),
    restAsync {},
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetUpdated {
    pub signing_key: String,
}

/// Suggest a better name
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asdf {
    pub interrupt_on_error: Option<bool>,
}

// `serde_yaml` doesn't support nested enums, thus this `singleton_map`
// workaround, see https://github.com/dtolnay/serde-yaml/issues/363#issuecomment-1478409196
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Execution {
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub condition: ExecutionCondition,
    pub targets: Vec<String>,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ExecutionCondition {
    request(#[serde(with = "serde_yaml::with::singleton_map")] RequestResponseCondition),
    response(#[serde(with = "serde_yaml::with::singleton_map")] RequestResponseCondition),
    function { name: String },
    event(#[serde(with = "serde_yaml::with::singleton_map")] EventCondition),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum RequestResponseCondition {
    method(String),
    service(String),
    all(TrueConst),
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(feature = "zitadel-rust-client")]
use {
    crate::{impl_traced_from, Traced},
    anyhow::{anyhow, Context},
    famedly_rust_utils::GenericCombinators,
    futures::stream::StreamExt,
    zitadel_rust_client::v2::management::*,
};

#[cfg(feature = "zitadel-rust-client")]
impl_traced_from!(anyhow::Error);

#[cfg(feature = "zitadel-rust-client")]
impl ZitadelInterface for zitadel_rust_client::v2::Zitadel {
    type Err = Traced<anyhow::Error>;
}

#[cfg(feature = "zitadel-rust-client")]
impl ZitadelHandleCreateOnly for zitadel_rust_client::v2::Zitadel {
    #[tracing::instrument(skip(self), level = "error")]
    async fn create_action(
        &self,
        action: ActionCreate,
        org_id: Option<String>,
    ) -> Result<String, Self::Err> {
        Ok(self
            .create_action(action.into(), org_id)
            .await?
            .id()
            .cloned()
            .context("Response missing id field")?)
    }

    #[tracing::instrument(skip(self), level = "error")]
    async fn set_trigger_actions(
        &self,
        flow_type: &str,
        trigger_type: &str,
        action_ids: Vec<String>,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        let flow_type = flow_type.parse::<u32>().context(FLOW_TRIGGER_FORMAT_ERR)?;
        let trigger_type = trigger_type.parse::<u32>().context(FLOW_TRIGGER_FORMAT_ERR)?;
        self.set_trigger_actions(
            flow_type,
            trigger_type,
            ManagementServiceSetTriggerActionsBody::new().with_action_ids(action_ids),
            org_id,
        )
        .await?;
        Ok(())
    }
}

#[cfg(feature = "zitadel-rust-client")]
impl ZitadelHandle for zitadel_rust_client::v2::Zitadel {
    #[tracing::instrument(skip(self), level = "error")]
    async fn search_actions_by_name(
        &self,
        name: &str,
        org_id: Option<String>,
    ) -> Result<Option<ActionSearch>, Self::Err> {
        // TODO: explicitly set TEXT_QUERY_METHOD_EQUALS
        Ok(self
            .list_actions(
                org_id,
                None,
                Some(vec![V1ActionQuery::new()
                    .with_action_name_query(V1ActionNameQuery::new().with_name(name.into()))]),
            )?
            .next()
            .await
            .transpose()?
            .map(TryInto::try_into)
            .transpose()
            .map_err(|f| anyhow!("Response missing {f} field"))?)
    }

    #[tracing::instrument(skip(self), level = "error")]
    async fn update_action(
        &self,
        id: &str,
        action: ActionUpdate,
        org_id: Option<String>,
    ) -> Result<(), Self::Err> {
        self.update_action(id.into(), action.into(), org_id).await?;
        Ok(())
    }

    #[tracing::instrument(skip(self), level = "error")]
    async fn delete_action(&self, id: &str, org_id: Option<String>) -> Result<(), Self::Err> {
        self.delete_action(id.into(), org_id).await?;
        Ok(())
    }

    #[tracing::instrument(skip(self), level = "error")]
    async fn get_triggers(
        &self,
        flow_type: &str,
        org_id: Option<String>,
    ) -> Result<Vec<GetTriggersResFlowAction>, Self::Err> {
        let flow_type = flow_type.parse::<u32>().context(FLOW_TRIGGER_FORMAT_ERR)?;
        Ok(from_flow_response(self.get_flow(flow_type, org_id).await?)
            .map_err(|f| anyhow!("Response missing {f} field"))?)
    }
}

#[cfg(feature = "zitadel-rust-client")]
const FLOW_TRIGGER_FORMAT_ERR: &str =
    "zitadel_rust_client backend doesn't support non-numeric flow and trigger types";

#[cfg(feature = "zitadel-rust-client")]
impl TryFrom<V1Action> for ActionSearch {
    type Error = &'static str;
    fn try_from(a: V1Action) -> Result<ActionSearch, Self::Error> {
        Ok(ActionSearch {
            id: a.id().ok_or("id")?.into(),
            name: a.name().ok_or("name")?.into(),
            timeout: a.timeout().cloned(),
            allowed_to_fail: a.allowed_to_fail().copied(),
            script: a.script().ok_or("script")?.into(),
        })
    }
}

#[cfg(feature = "zitadel-rust-client")]
impl From<ActionCreate> for V1CreateActionRequest {
    fn from(a: ActionCreate) -> Self {
        Self::new(a.name, a.script)
            .chain_opt(a.timeout, Self::with_timeout)
            .chain_opt(a.allowed_to_fail, Self::with_allowed_to_fail)
    }
}

#[cfg(feature = "zitadel-rust-client")]
impl From<ActionUpdate> for ManagementServiceUpdateActionBody {
    fn from(a: ActionUpdate) -> Self {
        Self::new(a.name, a.script)
            .chain_opt(a.timeout, Self::with_timeout)
            .chain_opt(a.allowed_to_fail, Self::with_allowed_to_fail)
    }
}

#[cfg(feature = "zitadel-rust-client")]
fn from_flow_response(a: V1GetFlowResponse) -> Result<Vec<GetTriggersResFlowAction>, String> {
    let flow = a.flow().ok_or("flow")?;
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
                                .ok_or("trigger_type")?
                                .id()
                                .ok_or("trigger_type.id")?
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
                                    .map_err(|f| ["actions", f].join("."))
                            },
                        )?,
                    })
                })
                .collect::<Result<_, String>>()
                .map_err(|f| ["flow", "trigger_actions", &f].join("."))
        },
    )
}
