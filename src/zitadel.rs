#![allow(async_fn_in_trait)]

use serde::{Deserialize, Serialize};

use crate::{Action, LoadedScript};

#[trait_variant::make(ZitadelHandle: Send + Sync)]
pub trait ZitadelHandlePrototype {
    type Err;
    async fn search_actions_by_name(&self, name: &str) -> Result<Option<ActionSearch>, Self::Err>;
    async fn create_action(&self, action: ActionCreate) -> Result<String, Self::Err>;
    async fn update_action(&self, id: &str, action: ActionUpdate) -> Result<(), Self::Err>;
    async fn delete_action(&self, id: &str) -> Result<(), Self::Err>;

    async fn get_triggers(
        &self,
        flow_type: &str,
    ) -> Result<Vec<GetTriggersResFlowAction>, Self::Err>;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionUpdate {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_to_fail: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

impl ActionUpdate {
    #[must_use]
    pub fn new(name: String, action: Action<LoadedScript>) -> Self {
        Self {
            name,
            timeout: action.timeout,
            allowed_to_fail: action.allowed_to_fail,
            script: Some(action.script),
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
