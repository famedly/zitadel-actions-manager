#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use std::collections::BTreeMap as Map;

use as_variant::as_variant;
use serde::{Deserialize, Serialize};

use crate::zitadel::*;

pub mod zitadel;

/// Zitadel action definition
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Deleted {
	Deleted,
}

/// Helper function to construct deleted markers in code
#[must_use]
pub const fn deleted<Script>() -> ActionEnum<Script> {
	ActionEnum::Deleted(Deleted::Deleted)
}

/// A generic parameter to [Action] representing fully loaded script
pub type LoadedScript = String;
/// A generic parameter to [Action] representing optinal script that may need to
/// be loaded from file
pub type OptionallyLoadedScript = Option<String>;

/// Full set fo action definitions (actions.yaml)
pub type Actions<Script> = Map<String, ActionEnum<Script>>;
/// Full set fo flows definitions (flows.yaml)
pub type Flows = Map<String, Map<String, Vec<String>>>;

#[allow(clippy::future_not_send)]
pub async fn sync<Z: ZitadelHandle>(
	create_only: bool,
	zitadel: &Z,
	actions: Actions<LoadedScript>,
	flows: Flows,
) -> Result<(), Z::Err> {
	// 1. Fetch existing action from zitadel (by names referenced in `actions`)
	let mut pre_existing_actions: Map<String, ActionSearch> = Map::new();
	if !create_only {
		for name in actions.keys() {
			if let Some(action) = zitadel.search_actions_by_name(name).await? {
				pre_existing_actions.insert(name.clone(), action);
			}
		}
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
			if !action_is_same(&action, &their_action) {
				zitadel.update_action(&their_action.id, ActionUpdate::new(action)).await?;
			}
			existing_actions.insert(name, their_action.id);
		} else {
			let action_id = zitadel.create_action(ActionCreate::new(name.clone(), action)).await?;
			existing_actions.insert(name, action_id);
		}
	}

	// 3. Set actions triggers aka "Set trigger actions" in the zitadel doc
	for (flow_type, trigger_types) in flows.into_iter() {
		for (trigger_type, action_names) in trigger_types.into_iter() {
			let action_ids = action_names
				.into_iter()
				.filter_map(|name| Some(existing_actions.get(&name)?.clone()))
				.collect::<Vec<_>>();
			zitadel.set_trigger_actions(&flow_type, &trigger_type, action_ids).await?;
		}
	}

	// 4. Delete actions that are marked as `deleted`
	for id in names_to_delete
		.into_iter()
		.filter_map(|name| existing_actions.get(&name).cloned())
	{
		zitadel.delete_action(&id).await?;
	}
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
			Ok::<_,std::io::Error>((
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
				panic!("Action `{action_name}` is marked as deleted but is used in flows")
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
