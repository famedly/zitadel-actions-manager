#![cfg_attr(all(doc, not(doctest)), feature(doc_auto_cfg))]
#![allow(missing_docs, clippy::missing_docs_in_private_items)]
// SPDX-FileCopyrightText: 2026 Famedly GmbH
//
// SPDX-License-Identifier: Apache-2.0

//! Sync v1 and v2 Zitadel IdP actions defined in a declarative way.
//!
//! Depending on the scenario, you need to define the actions and triggers. You
//! can do that statically in code by just constructing `Actions<LoadedScript>`
//! and `Flows`:
//! ```
//! # use zitadel_actions_manager::{Action, LoadedScript, Actions, Flows};
//! let actions: Actions<LoadedScript> = [(
//! 	"action1".to_owned(),
//! 	Some(Action {
//! 		timeout: None,
//! 		allowed_to_fail: false,
//! 		script: "function action1(ctx, api) {}".to_owned(),
//! 	}),
//! )]
//! .into();
//! let flows: Flows =
//! 	[("2".into(), [("4".into(), vec!["action1".to_owned()])].into())]
//! 		.into();
//! ```
//! or you can [`load`] them from files:
//! ```
//! # use std::path::Path;
//! let (actions, flows) = zitadel_actions_manager::load(
//! 	&Path::new("example-actions"),
//! 	None,
//! 	None,
//! )
//! .unwrap();
//! ```
//! and then [`sync`] actions with the constructed actions and flows.
//!
//! If you need to sync v1 actions for multiple organizations, see `sync_v1`
//! function in `src/main.rs` for reference.
//!
//! This crate comes with two clients for zitadel, simple ad-hoc
//! [`SimpleZitadelClient`](simple_zitadel_client::SimpleZitadelClient) and
//! [`famedly_zitadel_rust_client::v2::Zitadel`]. To use your own client you
//! need to implement [`ZitadelHandleCreateOnly`], [`ZitadelHandle`] and
//! [`ZitadelHandleV2`] traits for your zitadel client.
//!
//! For v2 actions the flow is similar: first you need to define in code or load
//! from files two structures: [`v2::Targets`] and [`v2::Executions`] and then
//! run [`v2::sync`] function.

use std::{collections::BTreeMap as Map, fmt, fs::File, path::Path};

use as_variant::as_variant;
use famedly_rust_utils::GenericCombinators;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;
// https://github.com/tokio-rs/tracing/issues/2082
#[cfg(not(coverage))]
pub use tracing::instrument;
#[cfg(coverage)]
pub use tracing_instrument_mock::instrument;

use crate::zitadel::*;

#[doc(hidden)]
pub const DEFAULT_ACTIONS_FILE: &str = "actions.yaml";
#[doc(hidden)]
pub const DEFAULT_FLOWS_FILE: &str = "flows.yaml";

#[cfg(feature = "simple-client")]
pub mod simple_zitadel_client;
pub mod v2;
pub mod zitadel;

/// Zitadel v1 action definition
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Action<Script> {
	pub timeout: Option<String>,
	#[serde(default)]
	pub allowed_to_fail: bool,
	pub script: Script,
}

/// A generic parameter to [`Action`] representing fully loaded script
pub type LoadedScript = String;
/// A generic parameter to [`Action`] representing optional script that may need
/// to be loaded from file
pub type OptionallyLoadedScript = Option<String>;

/// Full set for action definitions (`actions.yaml`)
///
/// ```yaml
/// action1:
///   # string, optional, for the exact format dig the zitadel docs
///   timeout: 'timeout'
///   # bool, optional
///   allowedToFail: false
///   # string, optional, if not set a file action1.js will be sourced
///   script: |
///     function action1(ctx, api) {
///       ...
///     }
///
/// # action that needs to be deleted if it exists in zitadel
/// action2: null
/// ```
pub type Actions<Script> = Map<String, Option<Action<Script>>>;

/// Full set for flows definitions (`flows.yaml`)
///
/// ```yaml
/// FLOW_TYPE_EXTERNAL_AUTHENTICATION:
///   TRIGGER_TYPE_PRE_CREATION: [action1]
/// ```
pub type Flows = Map<String, Map<String, Vec<String>>>;

/// Syncs v1 provided loaded actions and flows with running Zitadel instance.
/// Actions marked as `None` will be removed.
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
				&& trigger
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

			info!(%flow_type, %trigger_type, ?action_ids, "Setting actions trigger");
			zitadel
				.set_trigger_actions(&flow_type, &trigger_type, action_ids, org_id.clone())
				.await?;
		}
	}

	// 4. Delete actions that are marked as `deleted`
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

#[instrument]
pub fn load(
	dir: &Path,
	actions: Option<&Path>,
	flows: Option<&Path>,
) -> Result<(Actions<LoadedScript>, Flows), LoadActionsV1Error> {
	let flows_fname = dir.join(flows.unwrap_or(Path::new(DEFAULT_FLOWS_FILE)));
	let actions_fname = dir.join(actions.unwrap_or(Path::new(DEFAULT_ACTIONS_FILE)));
	let flows = from_yaml_file(&flows_fname)?;

	let actions = if std::fs::exists(&actions_fname)
		.with_context(|_| FileExistVerification { path: actions_fname.clone() })?
	{
		from_yaml_file(&actions_fname)?
	} else {
		info!(
			"File {actions_fname:?} doesn't exist, reading only actions referenced in {flows_fname:?}"
		);
		Actions::default()
	};
	let loaded_actions = load_actions(dir, actions, &flows)?;

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
) -> Result<Actions<LoadedScript>, LoadActionsV1Error> {
	use std::io::Read;
	let load_script = |name: &str| {
		tracing::info_span!("load_script", %name).in_scope(|| {
			let mut script = String::new();
			let full_path = dir.join([name, ".js"].concat());
			File::open(&full_path)
				.with_context(|_| OpenFile { path: full_path.clone() })?
				.read_to_string(&mut script)
				.with_context(|_| ReadFile { path: full_path })?;
			Ok::<_, LoadActionsV1Error>(script)
		})
	};
	let mut actions: Actions<LoadedScript> = actions
		.into_iter()
		.map(|(name, action)| {
			Ok::<_, LoadActionsV1Error>((
				name.clone(),
				action
					.map(|action| {
						let script = action.script.map_or_else(|| load_script(&name), Ok)?;
						Ok::<_, LoadActionsV1Error>(Action {
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
			action.as_ref().context(DeletedActionInFlow { action_name: action_name.to_owned() })?;
		} else {
			let loaded_action = Some(Action {
				timeout: None,
				allowed_to_fail: false,
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
) -> Result<T, ReadYamlFileError> {
	serde_yaml::from_reader(
		File::open(&path).context(OpenFile { path: path.as_ref().to_path_buf() })?,
	)
	.context(Parsing { path: path.as_ref().to_path_buf() })
}

use tracing_error::SpanTrace;

#[derive(Debug, Clone)]
pub struct SpanTraceWrapper(SpanTrace);

impl snafu::GenerateImplicitData for SpanTraceWrapper {
	fn generate() -> Self {
		Self(SpanTrace::capture())
	}
}

impl fmt::Display for SpanTraceWrapper {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.0.status() == tracing_error::SpanTraceStatus::CAPTURED {
			writeln!(f, "\nAt:")?;
			self.0.fmt(f)?;
			writeln!(f)?;
		}
		Ok(())
	}
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub), context(suffix(false)))]
pub enum ReadYamlFileError {
	#[snafu(display("Parsing yaml error. File: {}", path.to_string_lossy().to_string()))]
	Parsing {
		path: std::path::PathBuf,
		source: serde_yaml::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("Can't verify if the file exists. File: {}", path.to_string_lossy().to_string()))]
	FileExistVerification {
		path: std::path::PathBuf,
		source: std::io::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("Can't read the file. File: {}", path.to_string_lossy().to_string()))]
	ReadFile {
		path: std::path::PathBuf,
		source: std::io::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("Can't open the file. File: {}", path.to_string_lossy().to_string()))]
	OpenFile {
		path: std::path::PathBuf,
		source: std::io::Error,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
}

impl ReadYamlFileError {
	#[must_use]
	pub fn get_context(&self) -> &SpanTraceWrapper {
		match self {
			Self::Parsing { context, .. } => context,
			Self::FileExistVerification { context, .. } => context,
			Self::ReadFile { context, .. } => context,
			Self::OpenFile { context, .. } => context,
		}
	}
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub), context(suffix(false)))]
pub enum LoadActionsV1Error {
	#[snafu(display(
		"Action is marked as `null` (deleted) but is used in flows. Action: {action_name}"
	))]
	DeletedActionInFlow {
		action_name: String,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
	#[snafu(display("Reading yaml file error"))]
	ReadYamlFileError {
		source: ReadYamlFileError,
		#[snafu(implicit)]
		context: SpanTraceWrapper,
	},
}

impl From<ReadYamlFileError> for LoadActionsV1Error {
	fn from(error: ReadYamlFileError) -> Self {
		Self::ReadYamlFileError { context: error.get_context().clone(), source: error }
	}
}

impl LoadActionsV1Error {
	#[must_use]
	pub fn get_context(&self) -> &SpanTraceWrapper {
		match self {
			Self::ReadYamlFileError { context, .. } => context,
			Self::DeletedActionInFlow { context, .. } => context,
		}
	}
}
