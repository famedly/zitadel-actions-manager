#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use std::collections::BTreeMap as Map;

use as_variant::as_variant;
use famedly_rust_utils::GenericCombinators;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::zitadel::*;

#[cfg(feature = "simple-client")]
pub mod simple_zitadel_client;
pub mod zitadel;

/// Zitadel action definition
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Action<Script> {
    pub timeout: Option<String>,
    pub allowed_to_fail: Option<bool>,
    pub script: Script,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionEnum<Script> {
    /// An action that has to be created or updated
    Existing(Action<Script>),
    /// An action that has to be deleted if it exists
    Deleted(Deleted),
}

/// This type is only needed for [ActionEnum] serde. Use [deleted] function to
/// construct this in code
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Deleted {
    deleted,
}

/// Helper function to construct deleted markers in code
#[must_use]
pub const fn deleted<Script>() -> ActionEnum<Script> {
    ActionEnum::Deleted(Deleted::deleted)
}

/// A generic parameter to [Action] representing fully loaded script
pub type LoadedScript = String;
/// A generic parameter to [Action] representing optional script that may need
/// to be loaded from file
pub type OptionallyLoadedScript = Option<String>;

/// Full set for action definitions (actions.yaml)
pub type Actions<Script> = Map<String, ActionEnum<Script>>;
/// Full set for flows definitions (flows.yaml)
pub type Flows = Map<String, Map<String, Vec<String>>>;

#[instrument(skip_all, level = "error")]
pub async fn sync<Z: ZitadelHandle>(
    create_only: bool,
    zitadel: &Z,
    actions: Actions<LoadedScript>,
    flows: Flows,
) -> Result<(), Z::Err> {
    // 1. Fetch existing action from zitadel (by names referenced in `actions`)
    let mut pre_existing_actions: Map<String, ActionSearch> = Map::new();
    if !create_only {
        info!("Fetching all locally defined actions by their names");
        for name in actions.keys() {
            if let Some(action) = zitadel.search_actions_by_name(name).await? {
                pre_existing_actions.insert(name.clone(), action);
            }
        }
        info!(
            "Fetched {} actions out of {} defined locally",
            pre_existing_actions.len(),
            actions.len()
        );
    }

    let names_to_delete = actions
        .iter()
        .filter_map(|(name, action)| as_variant!(action, ActionEnum::Deleted(_) => name.into()))
        .collect::<Vec<String>>();
    let actions_to_update = actions.into_iter().filter_map(
        |(name, action)| as_variant!(action, ActionEnum::Existing(action) => (name, action)),
    );

    // 2. Create and update actions
    let mut existing_actions = Map::new();
    for (name, action) in actions_to_update {
        if let Some(their_action) = pre_existing_actions.remove(&name) {
            if action_is_same(&action, &their_action) {
                info!(%name, action_id = %their_action.id, "Action is unchanged, skipping");
            } else {
                info!(%name, action_id = %their_action.id, "Updating action");
                zitadel
                    .update_action(&their_action.id, ActionUpdate::new(name.clone(), action))
                    .await?;
            }
            existing_actions.insert(name, their_action.id);
        } else {
            info!(%name, "New action detected, creating");
            let action_id = zitadel.create_action(ActionCreate::new(name.clone(), action)).await?;
            info!(%name, %action_id, "Created action");
            existing_actions.insert(name, action_id);
        }
    }

    // 3. Set actions triggers aka "Set trigger actions" in the zitadel doc
    for (flow_type, trigger_types) in flows.into_iter() {
        // We need to check if triggers have changed, otherwise zitadel call fails
        let triggers = zitadel.get_triggers(&flow_type).await?;
        for (trigger_type, action_names) in trigger_types.into_iter() {
            let action_ids = action_names
                .into_iter()
                .filter_map(|name| Some(existing_actions.get(&name)?.clone()))
                .collect::<Vec<_>>()
                .mutate(|ids| ids.sort()); // TODO: figure out if actions order in a trigger matters

            if let Some(trigger) =
                triggers.iter().find(|trigger| trigger.trigger_type.id == trigger_type)
            {
                if trigger
                    .actions
                    .iter()
                    .map(|action| action.id.clone())
                    .collect::<Vec<_>>()
                    .mutate(|ids| ids.sort())
                    == action_ids
                {
                    info!(%flow_type, %trigger_type, ?action_ids, "Triggers are unchanged, skipping");
                    break;
                }
            }

            info!(%flow_type, %trigger_type, ?action_ids, "Setting actions trigger");
            zitadel.set_trigger_actions(&flow_type, &trigger_type, action_ids).await?;
        }
    }

    // 4. Delete actions that are marked as `deleted`
    for (id, name) in names_to_delete
        .into_iter()
        .filter_map(|name| Some((existing_actions.get(&name)?.clone(), name)))
    {
        info!(%id, %name, "Deleting action");
        zitadel.delete_action(&id).await?;
    }

    info!("Sync successful");
    Ok(())
}

/// Takes a map of actions with possibly missing `script` fields. If that fields
/// is missing it reads `{action_name}.js` file and returns the same map but
/// with all `script` fields filled out. The same way actions mentioned in Flows
/// are loaded, so Actions can actually be an empty map with only Flows not
/// empty
pub fn load_actions(
    dir: &str,
    actions: Actions<OptionallyLoadedScript>,
    flows: &Flows,
) -> Result<Actions<LoadedScript>, std::io::Error> {
    use std::io::Read;
    let load_script = |name: &str| {
        let mut script = String::new();
        std::fs::File::open(format!("{dir}/{name}.js"))?.read_to_string(&mut script)?;
        Ok::<_, std::io::Error>(script)
    };
    let mut actions: Actions<LoadedScript> = actions
        .into_iter()
        .map(|(name, action)| {
            Ok::<_, std::io::Error>((
                name.clone(),
                match action {
                    ActionEnum::Existing(action) => {
                        let script = action.script.map_or_else(|| load_script(&name), Ok)?;
                        ActionEnum::Existing(Action {
                            timeout: action.timeout,
                            allowed_to_fail: action.allowed_to_fail,
                            script,
                        })
                    }
                    ActionEnum::Deleted(d) => ActionEnum::Deleted(d),
                },
            ))
        })
        .collect::<Result<Map<_, _>, _>>()?;

    for action_name in flows.values().flat_map(|x| x.values().flat_map(|v| v.iter())) {
        if let Some(action) = actions.get(action_name) {
            if matches!(action, ActionEnum::Deleted(_)) {
                return Err(std::io::Error::other(format!(
                    "Action `{action_name}` is marked as deleted but is used in flows"
                )));
            }
        } else {
            let loaded_action = ActionEnum::Existing(Action {
                timeout: None,
                allowed_to_fail: None,
                script: load_script(action_name)?,
            });
            actions.insert(action_name.clone(), loaded_action);
        }
    }
    Ok(actions)
}

// TODO: factor out into `famedly_rust_utils`:

#[derive(Debug, thiserror::Error)]
pub struct Traced<E> {
    error: E,
    span: tracing_error::SpanTrace,
}

use std::fmt;

impl<E: fmt::Display> fmt::Display for Traced<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)?;
        if self.span.status() == tracing_error::SpanTraceStatus::CAPTURED {
            write!(f, "\nAt:\n")?;
            self.span.fmt(f)?;
        }
        Ok(())
    }
}
