// SPDX-FileCopyrightText: 2026 Famedly GmbH
//
// SPDX-License-Identifier: Apache-2.0

use std::{any, marker::PhantomData};

#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::Zitadel;
use serde::Deserialize;
use serde_json::json;
use snafu::{OptionExt as _, ResultExt as _};
use test_case::test_case;
use tokio::fs;
use tracing_test::traced_test;
use url::Url;
use wiremock::{
	Mock, ResponseTemplate,
	matchers::{method, path, path_regex},
};
#[cfg(feature = "simple-client")]
use zitadel_actions_manager::simple_zitadel_client::SimpleZitadelClient;
use zitadel_actions_manager::{load, sync};

use super::{Result, TestContext, TestZitadelHandle, assert_context_msg, create_context};
use crate::e2e::get_zitadel_mock;

async fn test_v1<T: TestZitadelHandle>(
	actions_content: &str,
	flows_content: &str,
	context: Option<TestContext<T>>,
) -> Result<()> {
	let context = context.unwrap_or(create_context::<T>(None, false).await);
	let actions_path = context.path.path().join("actions.yaml");
	let flows_path = context.path.path().join("flows.yaml");
	fs::write(actions_path.clone(), actions_content)
		.await
		.whatever_context("Error writing actions")?;
	fs::write(flows_path.clone(), flows_content).await.whatever_context("Error writing flows")?;

	let (actions, flows) =
		load(context.path.path(), Some(actions_path.as_path()), Some(flows_path.as_path()))
			.whatever_context("Error loading actions and flows")?;

	sync(Some(context.org_id.clone()), &context.zitadel_handle, actions.clone(), flows.clone())
		.await
		.with_whatever_context(|_| {
			format!("{}: Error syncing the actions", any::type_name::<T>())
		})?;

	for (action_name, action) in actions.iter() {
		let synced_action = context
			.zitadel_handle
			.search_actions_by_name(action_name, Some(context.org_id.clone()))
			.await
			.whatever_context("Error searching actions by name")?
			.with_whatever_context(|| {
				format!(
					"{}: Action '{action_name}' not found in zitadel org {}",
					any::type_name::<T>(),
					context.org_id
				)
			});

		if action.is_none() {
			assert!(
				synced_action.is_err(),
				"{}: Action '{action_name}' should be deleted",
				any::type_name::<T>()
			);
			continue;
		}

		let synced_action = synced_action?;
		assert_eq!(&synced_action.name, action_name, "{}", assert_context_msg::<T>());
		assert_eq!(
			synced_action.script,
			action.as_ref().unwrap().script,
			"{}",
			assert_context_msg::<T>()
		);
		assert_eq!(
			synced_action.timeout,
			action.as_ref().unwrap().timeout,
			"{}",
			assert_context_msg::<T>()
		);
		// Zitadel is returning None if allowed_to_fail is false
		assert_eq!(
			synced_action.allowed_to_fail,
			action.as_ref().unwrap().allowed_to_fail,
			"{}",
			assert_context_msg::<T>()
		);
	}

	for (flow_name, flow) in flows.iter() {
		let synced_flow = context
			.zitadel_handle
			.get_triggers(flow_name, Some(context.org_id.clone()))
			.await
			.whatever_context("Error getting triggers")?;
		for (trigger_type, trigger_actions) in flow.iter() {
			let synced_trigger_actions = synced_flow
				.iter()
				.find(|t| &t.trigger_type.id == trigger_type)
				.map(|t| t.actions.iter().map(|a| a.name.clone()).collect::<Vec<_>>())
				.with_whatever_context(|| {
					format!(
						"{}: Trigger '{trigger_type}' not found in zitadel org {}",
						any::type_name::<T>(),
						context.org_id
					)
				})?;
			assert_eq!(&synced_trigger_actions, trigger_actions, "{}", assert_context_msg::<T>());
		}
	}

	Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_simple<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
	let flows = r#"
    2:
        4: [action1]
    "#;
	let actions = r#"
    action1:
        timeout: '10s'
        allowedToFail: true
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;
	test_v1::<T>(actions, flows, None).await?;

	Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_resync<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
	let flows = r#"
    2:
        4: [action1]
    "#;
	let actions = r#"
    action1:
        timeout: '10s'
        allowedToFail: true
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;

	let context = create_context::<T>(None, false).await;
	test_v1::<T>(actions, flows, Some(context.clone())).await?;
	test_v1::<T>(actions, flows, Some(context)).await?;

	Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_update<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
	let flows = r#"
    2:
        4: [action1]
    "#;
	let actions = r#"
    action1:
        timeout: '10s'
        allowedToFail: true
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;

	let new_flows = r#"
    2:
        5: [action1]
    "#;
	let new_actions = r#"
    action1:
        timeout: '12s'
        allowedToFail: false
        script: |
            function action1(ctx, api) {
                console.log("new action1");
            }
    "#;

	let context = create_context::<T>(None, false).await;
	test_v1::<T>(actions, flows, Some(context.clone())).await?;
	test_v1::<T>(new_actions, new_flows, Some(context)).await?;

	Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_remove<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
	let flows = r#"
    2:
        4: [action1]
    "#;
	let actions = r#"
    action1:
        timeout: '10s'
        allowedToFail: true
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;

	let new_flows = r#"
    "#;
	let new_actions = r#"
    action1: null
    "#;

	let context = create_context::<T>(None, false).await;
	test_v1::<T>(actions, flows, Some(context.clone())).await?;
	test_v1::<T>(new_actions, new_flows, Some(context)).await?;

	Ok(())
}

static TEST_CASE_FALSE: &str = r#"
    action1:
        timeout: '10s'
        allowedToFail: false
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;
static TEST_CASE_TRUE: &str = r#"
    action1:
        timeout: '10s'
        allowedToFail: true
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;
static TEST_CASE_NONE: &str = r#"
    action1:
        timeout: '10s'
        script: |
            function action1(ctx, api) {
                console.log("action1");
            }
    "#;

#[allow(clippy::too_many_lines)]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(TEST_CASE_FALSE, PhantomData::<Zitadel>; "allowedToFail_false::zrc"))]
#[cfg_attr(feature = "simple-client",test_case(TEST_CASE_FALSE, PhantomData::<SimpleZitadelClient>; "allowedToFail_false:szc"))]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(TEST_CASE_TRUE, PhantomData::<Zitadel>; "allowedToFail_true:zrc"))]
#[cfg_attr(feature = "simple-client",test_case(TEST_CASE_TRUE, PhantomData::<SimpleZitadelClient>; "allowedToFail_true:szc"))]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(TEST_CASE_NONE, PhantomData::<Zitadel>; "allowedToFail_none:zrc"))]
#[cfg_attr(feature = "simple-client",test_case(TEST_CASE_NONE, PhantomData::<SimpleZitadelClient>; "allowedToFail_none:szc"))]
#[tokio::test]
#[traced_test]
async fn test_resync_only_once<T: TestZitadelHandle>(
	actions_str: &str,
	_: PhantomData<T>,
) -> Result<()> {
	#[derive(Deserialize)]
	struct Actions {
		action1: Action,
	}
	#[derive(Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Action {
		timeout: String,
		allowed_to_fail: Option<bool>,
		script: String,
	}

	let mock_server = get_zitadel_mock().await;
	let context = create_context::<T>(
		Some(&Url::parse(&mock_server.uri()).expect("Error parsing mock zitadel url")),
		false,
	)
	.await;

	let flows = r#"
    2:
        4: [action1]
    "#;

	let actions: Actions =
		serde_yaml::from_str(actions_str).whatever_context("Error parsing actions")?;

	Mock::given(method("POST"))
		.and(path_regex(r"management/v1/flows/.*/trigger/.*"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
		.expect(1)
		.named("create trigger actions")
		.mount(&mock_server)
		.await;

	Mock::given(method("POST"))
		.and(path("management/v1/actions"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({
			"id": "test"
		})))
		.expect(1)
		.named("create action")
		.mount(&mock_server)
		.await;

	Mock::given(method("PUT"))
		.and(path_regex(r"management/v1/actions/.*"))
		.respond_with(ResponseTemplate::new(200))
		.expect(0)
		.named("update action")
		.mount(&mock_server)
		.await;

	Mock::given(method("DELETE"))
		.and(path_regex(r"management/v1/actions/.*"))
		.respond_with(ResponseTemplate::new(200))
		.expect(0)
		.named("delete action")
		.mount(&mock_server)
		.await;

	// First request - empty result
	Mock::given(method("POST"))
		.and(path("management/v1/actions/_search"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({
			"result": []
		})))
		.up_to_n_times(1)
		.named("search action - empty")
		.mount(&mock_server)
		.await;

	let test_action = if let Some(allowed_to_fail) = actions.action1.allowed_to_fail {
		json!({
			"id": "test",
			"name": "action1",
			"timeout": actions.action1.timeout,
			"allowedToFail":allowed_to_fail,
			"script": actions.action1.script
		})
	} else {
		json!({
			"id": "test",
			"name": "action1",
			"timeout": actions.action1.timeout,
			"script": actions.action1.script
		})
	};

	tracing::info!("test_action: {:?}", test_action);

	// Subsequent requests - with data
	Mock::given(method("POST"))
		.and(path("management/v1/actions/_search"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({
			"result": [
				test_action
			]
		})))
		.named("search action - with data")
		.mount(&mock_server)
		.await;

	Mock::given(method("GET"))
		.and(path_regex(r"management/v1/flows/.*"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({
			"flow": {
				"type": {"id": "2"},
			}
		})))
		.up_to_n_times(1)
		.named("get triggers - empty")
		.mount(&mock_server)
		.await;

	Mock::given(method("GET"))
		.and(path_regex(r"management/v1/flows/.*"))
		.respond_with(ResponseTemplate::new(200).set_body_json(json!({
			"flow": {
				"type": {"id": "2"},
				"triggerActions": [ {
					"triggerType": {"id": "4"},
					"actions": [
						test_action
					]
				}
				]
			}
		})))
		.named("get triggers - with data")
		.mount(&mock_server)
		.await;

	test_v1::<T>(actions_str, flows, Some(context.clone())).await?;
	test_v1::<T>(actions_str, flows, Some(context))
		.await
		.inspect_err(|e| tracing::error!("{:?}", e))?;

	Ok(())
}
