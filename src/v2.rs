// SPDX-FileCopyrightText: 2026 Famedly GmbH
//
// SPDX-License-Identifier: Apache-2.0

//! Actions v2 sync.
use std::{collections::BTreeMap as Map, path::Path};

use as_variant::as_variant;
use famedly_rust_utils::GenericCombinators;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
	ReadYamlFileError, from_yaml_file, instrument,
	zitadel::{CreateTarget, Execution, FoundTarget, TargetType, UpdateTarget, ZitadelHandleV2},
};

#[doc(hidden)]
pub const DEFAULT_TARGETS_FILE: &str = "targets.yaml";
#[doc(hidden)]
pub const DEFAULT_EXECUTIONS_FILE: &str = "executions.yaml";

/// Targets definitions (`targets.yaml`)
///
/// ```yaml
/// target1:
///   restAsync: {}
///   endpoint: http://example.com/call_me
///   timeout: 5s
///
/// # delete target2 target if it exists
/// target2: null
/// ```
pub type Targets = Map<String, Option<Target>>;

/// Execution definitions (`executions.yaml`)
///
/// ```yaml
/// - condition: {event: {event: user.human.added}}
///   targets: [target1] # targets by their names defined in targets.yaml
///
/// - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
///   targets: [target1]
/// ```
pub type Executions = Vec<Execution>;

/// Target definition. Reflects HTTP API types exposed by Zitadel.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
	#[serde(flatten)]
	pub target_type: TargetType,
	pub timeout: String,
	pub endpoint: url::Url,
}

// TODO: it's very likely that zitadel returns some optional fields filled with
// default values, we should account for it, similar to zitadel::action_is_same
impl PartialEq<FoundTarget> for Target {
	fn eq(&self, other: &FoundTarget) -> bool {
		self.target_type == other.target_type
			&& self.timeout == other.timeout
			&& self.endpoint.as_str() == other.endpoint
	}
}

impl From<Target> for UpdateTarget {
	fn from(target: Target) -> Self {
		Self {
			target_type: Some(target.target_type),
			timeout: Some(target.timeout),
			endpoint: Some(target.endpoint.to_string()),
			expiration_signing_key: None,
		}
	}
}

impl Target {
	#[must_use]
	pub fn into_create_target(self, name: String) -> CreateTarget {
		CreateTarget {
			name,
			target_type: self.target_type,
			timeout: self.timeout,
			endpoint: self.endpoint.to_string(),
		}
	}
}

/// Syncs v2 provided loaded targets and executions with running Zitadel
/// instance. Targets marked as `None` will be deleted.
#[instrument(skip_all)]
pub async fn sync<Z: ZitadelHandleV2>(
	zitadel: &Z,
	targets: Targets,
	executions: Executions,
) -> Result<(), Z::Err> {
	// 1. Fetch existing Targets from zitadel (by names referenced in `targets`)
	let mut pre_existing_targets: Map<String, _> = Map::new();
	info!("Fetching all locally defined actions by their names");
	for name in targets.keys() {
		if let Some(action) = zitadel.search_target_by_name(name).await? {
			pre_existing_targets.insert(name.clone(), action);
		}
	}
	info!(
		"Fetched {} targets out of {} defined locally",
		pre_existing_targets.len(),
		targets.len()
	);

	let names_to_delete = targets
		.iter()
		.filter_map(|(name, target)| as_variant!(target, None => name.into()))
		.collect::<Vec<String>>();
	let targets_to_update = targets
		.into_iter()
		.filter_map(|(name, target)| as_variant!(target, Some(target) => (name, target)));

	// 2. Create and update Targets
	let mut existing_targets = Map::new();
	for (name, target) in targets_to_update {
		if let Some(their_target) = pre_existing_targets.remove(&name) {
			if target == their_target {
				info!(%name, target_id = %their_target.id, "Target is unchanged, skipping");
			} else {
				info!(%name, target_id = %their_target.id, "Updating target");
				zitadel.update_target(&their_target.id, target.into()).await?;
			}
			existing_targets.insert(name, their_target.id);
		} else {
			info!(%name, "New target detected, creating");

			let target_id =
				zitadel.create_target(target.into_create_target(name.clone())).await?.id;
			info!(%name, %target_id, "Created target");
			existing_targets.insert(name, target_id);
		}
	}

	// 3. Set Executions
	let mut existing_executions = zitadel.list_executions().await?;
	existing_executions.iter_mut().for_each(|execution| execution.targets.sort());
	for mut execution in executions.into_iter() {
		let target_ids = execution
			.targets
			.iter()
			.filter_map(|name| existing_targets.get(name).cloned())
			.collect::<Vec<_>>()
			.mutate(|ids| ids.sort()); // TODO: figure out if targets order in a execution matters
		let condition = &execution.condition;

		if let Some(existing_execution) =
			existing_executions.iter().find(|execution| &execution.condition == condition)
			&& existing_execution.targets == target_ids
		{
			info!(?condition, ?target_ids, "Triggers are unchanged, skipping");
			continue;
		}

		info!(?condition, ?target_ids, "Setting targets execution");
		execution.targets = target_ids;
		zitadel.set_execution(execution).await?;
	}

	// 4. Delete Targets that are marked as null
	for (id, name) in names_to_delete
		.into_iter()
		.filter_map(|name| Some((existing_targets.get(&name)?.clone(), name)))
	{
		info!(%id, %name, "Deleting action");
		zitadel.delete_target(&id).await?;
	}

	info!("Sync successful");

	Ok(())
}

/// Loads v2 targets and executions.
///
/// - `dir` is a directory path to where source targets and executions.
/// - `targets` is an optional path relative to `dir`, `targets.yaml` by
///   default.
/// - `executions` is an optional path relative to `dir`, `executions.yaml` by
///   default.
#[instrument]
pub fn load(
	dir: &Path,
	targets: Option<&Path>,
	executions: Option<&Path>,
) -> Result<(Targets, Executions), ReadYamlFileError> {
	let targets_fname = dir.join(targets.unwrap_or(Path::new(DEFAULT_TARGETS_FILE)));
	let executions_fname = dir.join(executions.unwrap_or(Path::new(DEFAULT_EXECUTIONS_FILE)));
	let targets = from_yaml_file(&targets_fname)?;
	let executions = from_yaml_file(&executions_fname)?;
	Ok((targets, executions))
}
