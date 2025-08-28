#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use std::{collections::BTreeMap as Map, fmt, fs::File, io::Error as IoError, path::Path};

use as_variant::as_variant;
use famedly_rust_utils::GenericCombinators;
#[cfg(coverage)]
pub use proc_macro_aliases::instrument;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::info;
// https://github.com/tokio-rs/tracing/issues/2082
#[cfg(not(coverage))]
pub use tracing::instrument;

use crate::zitadel::*;

pub const DEFAULT_ACTIONS_FILE: &str = "actions.yaml";
pub const DEFAULT_FLOWS_FILE: &str = "flows.yaml";

#[cfg(feature = "simple-client")]
pub mod simple_zitadel_client;
pub mod v2;
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

/// A generic parameter to [Action] representing fully loaded script
pub type LoadedScript = String;
/// A generic parameter to [Action] representing optional script that may need
/// to be loaded from file
pub type OptionallyLoadedScript = Option<String>;

/// Full set for action definitions (actions.yaml)
pub type Actions<Script> = Map<String, Option<Action<Script>>>;
/// Full set for flows definitions (flows.yaml)
pub type Flows = Map<String, Map<String, Vec<String>>>;

#[instrument(skip_all, fields(org_id))]
pub async fn sync<Z: ZitadelHandle>(
    org_id: Option<String>,
    zitadel: &Z,
    actions: Actions<LoadedScript>,
    flows: Flows,
) -> Result<(), Z::Err> {
    // 1. Fetch existing action from zitadel (by names referenced in `actions`)
    let mut pre_existing_actions: Map<String, ActionSearch> = Map::new();
    info!("Fetching all locally defined actions by their names");
    for name in actions.keys() {
        if let Some(action) = zitadel.search_actions_by_name(name, org_id.clone()).await? {
            pre_existing_actions.insert(name.clone(), action);
        }
    }
    info!(
        "Fetched {} actions out of {} defined locally",
        pre_existing_actions.len(),
        actions.len()
    );

    let names_to_delete = actions
        .iter()
        .filter_map(|(name, action)| as_variant!(action, None => name.clone()))
        .collect::<Vec<String>>();
    let actions_to_update = actions
        .into_iter()
        .filter_map(|(name, action)| as_variant!(action, Some(action) => (name, action)));

    // 2. Create and update actions
    let mut existing_actions = Map::new();
    for (name, action) in actions_to_update {
        if let Some(their_action) = pre_existing_actions.remove(&name) {
            if action_is_same(&action, &their_action) {
                info!(%name, action_id = %their_action.id, "Action is unchanged, skipping");
            } else {
                info!(%name, action_id = %their_action.id, "Updating action");
                zitadel
                    .update_action(
                        &their_action.id,
                        ActionUpdate::new(name.clone(), action),
                        org_id.clone(),
                    )
                    .await?;
            }
            existing_actions.insert(name, their_action.id);
        } else {
            info!(%name, "New action detected, creating");
            let action_id = zitadel
                .create_action(ActionCreate::new(name.clone(), action), org_id.clone())
                .await?;
            info!(%name, %action_id, "Created action");
            existing_actions.insert(name, action_id);
        }
    }

    // 3. Set actions triggers aka "Set trigger actions" in the zitadel doc
    for (flow_type, trigger_types) in flows.into_iter() {
        // We need to check if triggers have changed, otherwise zitadel call fails
        let existing_triggers = zitadel.get_triggers(&flow_type, org_id.clone()).await?;

        for (trigger_type, action_names) in trigger_types.into_iter() {
            let action_ids = action_names
                .into_iter()
                .filter_map(|name| Some(existing_actions.get(&name)?.clone()))
                .collect::<Vec<_>>()
                .mutate(|ids| ids.sort()); // TODO: figure out if actions order in a trigger matters

            if let Some(trigger) =
                existing_triggers.iter().find(|trigger| trigger.trigger_type.id == trigger_type)
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
                    continue;
                }
            }

            info!(%flow_type, %trigger_type, ?action_ids, "Setting actions trigger");
            zitadel
                .set_trigger_actions(&flow_type, &trigger_type, action_ids, org_id.clone())
                .await?;
        }
    }

    // 4. Delete actions that are marked as `null`
    for action in names_to_delete.into_iter().filter_map(|name| pre_existing_actions.get(&name)) {
        info!(id = action.id, name = action.name, "Deleting action");
        zitadel.delete_action(&action.id, org_id.clone()).await?;
    }

    info!("Sync successful");
    Ok(())
}

/// This should be used only for newly created organizations or fresh instances.
/// The zitadel may return an error if there are already existing actions or
/// triggers
#[instrument(skip_all, fields(org_id))]
pub async fn create_only<Z: ZitadelHandleCreateOnly>(
    org_id: Option<String>,
    zitadel: &Z,
    actions: Actions<LoadedScript>,
    flows: Flows,
) -> Result<(), Z::Err> {
    let actions_to_create = actions
        .into_iter()
        .filter_map(|(name, action)| as_variant!(action, Some(action) => (name, action)));

    // 1. Create new actions
    let mut existing_actions = Map::new();
    for (name, action) in actions_to_create {
        let action_id =
            zitadel.create_action(ActionCreate::new(name.clone(), action), org_id.clone()).await?;
        info!(%name, %action_id, "Created action");
        existing_actions.insert(name, action_id);
    }

    // 2. Set actions triggers aka "Set trigger actions" in the zitadel doc
    for (flow_type, trigger_types) in flows.into_iter() {
        for (trigger_type, action_names) in trigger_types.into_iter() {
            let action_ids = action_names
                .into_iter()
                .filter_map(|name| Some(existing_actions.get(&name)?.clone()))
                .collect::<Vec<_>>()
                .mutate(|ids| ids.sort()); // TODO: figure out if actions order in a trigger matters

            info!(%flow_type, %trigger_type, ?action_ids, "Setting actions trigger");
            zitadel
                .set_trigger_actions(&flow_type, &trigger_type, action_ids, org_id.clone())
                .await?;
        }
    }

    info!("Sync successful");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ReadYamlFileError {
    #[error("IO error: {0}")]
    Io(#[from] IoError),
    #[error("Parsing yaml error: {0}")]
    Parsing(#[from] serde_yaml::Error),
}

#[instrument]
pub fn load(
    dir: &Path,
    actions: Option<&Path>,
    flows: Option<&Path>,
) -> Result<(Actions<LoadedScript>, Flows), Traced<ReadYamlFileError>> {
    let flows_fname = dir.join(flows.unwrap_or(Path::new(DEFAULT_FLOWS_FILE)));
    let actions_fname = dir.join(actions.unwrap_or(Path::new(DEFAULT_ACTIONS_FILE)));
    let flows = from_yaml_file(&flows_fname)?;

    let actions = if std::fs::exists(&actions_fname)
        .map_err(ReadYamlFileError::from)
        .map_err(Traced::new)?
    {
        from_yaml_file(&actions_fname)?
    } else {
        info!("File {actions_fname:?} doesn't exist, reading only actions referenced in {flows_fname:?}");
        Actions::default()
    };
    let loaded_actions = load_actions(dir, actions, &flows).map_err(Traced::map_from)?;

    Ok((loaded_actions, flows))
}

/// Takes a map of actions with possibly missing `script` fields. If that fields
/// is missing it reads `{action_name}.js` file and returns the same map but
/// with all `script` fields filled out. The same way actions mentioned in Flows
/// are loaded, so Actions can actually be an empty map with only Flows not
/// empty
pub fn load_actions(
    dir: &Path,
    actions: Actions<OptionallyLoadedScript>,
    flows: &Flows,
) -> Result<Actions<LoadedScript>, Traced<IoError>> {
    use std::io::Read;
    let load_script = |name: &str| {
        tracing::info_span!("load_script", %name).in_scope(|| {
            let mut script = String::new();
            File::open(dir.join([name, ".js"].concat()))
                .map_err(Traced::new)?
                .read_to_string(&mut script)
                .map_err(Traced::new)?;
            Ok::<_, Traced<IoError>>(script)
        })
    };
    let mut actions: Actions<LoadedScript> = actions
        .into_iter()
        .map(|(name, action)| {
            Ok::<_, Traced<IoError>>((
                name.clone(),
                action
                    .map(|action| {
                        let script = action.script.map_or_else(|| load_script(&name), Ok)?;
                        Ok(Action {
                            timeout: action.timeout,
                            allowed_to_fail: action.allowed_to_fail,
                            script,
                        })
                    })
                    .transpose()?,
            ))
        })
        .collect::<Result<Map<_, _>, _>>()?;

    for action_name in flows.values().flat_map(|x| x.values().flat_map(|v| v.iter())) {
        if let Some(action) = actions.get(action_name) {
            action.as_ref().ok_or_else(|| {
                Traced::new(IoError::other(format!(
                    "Action `{action_name}` is marked as `null` (deleted) but is used in flows"
                )))
            })?;
        } else {
            let loaded_action = Some(Action {
                timeout: None,
                allowed_to_fail: None,
                script: load_script(action_name)?,
            });
            actions.insert(action_name.to_owned(), loaded_action);
        }
    }
    Ok(actions)
}

#[doc(hidden)]
#[instrument]
pub fn from_yaml_file<T: DeserializeOwned, P: fmt::Debug + AsRef<Path>>(
    path: P,
) -> Result<T, Traced<ReadYamlFileError>> {
    serde_yaml::from_reader(File::open(path).map_err(ReadYamlFileError::from).map_err(Traced::new)?)
        .map_err(ReadYamlFileError::from)
        .map_err(Traced::new)
}

// TODO: factor out into `famedly_rust_utils`:

#[derive(Debug, thiserror::Error)]
pub struct Traced<E> {
    pub error: E,
    pub span: tracing_error::SpanTrace,
}

impl<E> Traced<E> {
    pub fn new(error: E) -> Self {
        Self { error, span: tracing_error::SpanTrace::capture() }
    }

    pub fn map_from<Y: From<E>>(self) -> Traced<Y> {
        Traced { error: self.error.into(), span: self.span }
    }
}

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

// until marker traits are stable, (need impls overloading)
#[macro_export]
macro_rules! impl_traced_from {
    ($($t:ty),+) => {
        $(
        impl From<$t> for Traced<$t> {
            fn from(error: $t) -> Self {
                Self { error, span: tracing_error::SpanTrace::capture() }
            }
        }
        )+
    }
}

impl_traced_from!(Box<dyn std::error::Error>);
