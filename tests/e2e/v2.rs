use std::{any, collections::HashMap, marker::PhantomData};

use anyhow::{Context, Result};
use famedly_zitadel_rust_client::v2::actions::V2betaListExecutionsRequest;
#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::Zitadel;
use serde_json::json;
use test_case::test_case;
use tokio::fs;
use tracing_test::traced_test;
use url::Url;
use wiremock::{
    matchers::{method, path, path_regex},
    Mock, ResponseTemplate,
};
#[cfg(feature = "simple-client")]
use zitadel_actions_manager::simple_zitadel_client::SimpleZitadelClient;
use zitadel_actions_manager::v2;

use super::{assert_context_msg, create_context, TestContext, TestZitadelHandle};
use crate::e2e::get_zitadel_mock;

async fn test_v2<T: TestZitadelHandle>(
    targets_content: &str,
    executions_content: &str,
    context: Option<TestContext<T>>,
) -> Result<()> {
    let context = context.unwrap_or(create_context::<T>(None, true).await);
    let targets_path = context.path.path().join("targets.yaml");
    let executions_path = context.path.path().join("executions.yaml");
    fs::write(targets_path.clone(), targets_content).await?;
    fs::write(executions_path.clone(), executions_content).await?;

    let (targets, executions) = v2::load(
        context.path.path(),
        Some(targets_path.as_path()),
        Some(executions_path.as_path()),
    )?;

    v2::sync(&context.zitadel_handle, targets.clone(), executions.clone())
        .await
        .context("Error syncing the targets")?;

    let mut targets_map = HashMap::new();

    for (target_name, target) in targets.iter() {
        let synced_target =
            context.zitadel_handle.search_target_by_name(target_name).await?.with_context(|| {
                format!("{}: Target '{target_name}' not found in zitadel", any::type_name::<T>())
            });

        if target.is_none() {
            assert!(
                synced_target.is_err(),
                "{}: Target '{target_name}' should be deleted",
                any::type_name::<T>()
            );
            continue;
        }

        let synced_target = synced_target?;
        targets_map.insert(synced_target.id, target_name.clone());

        assert_eq!(&synced_target.name, target_name, "{}", assert_context_msg::<T>());
        assert_eq!(
            synced_target.target_type,
            target.as_ref().unwrap().target_type,
            "{}",
            assert_context_msg::<T>()
        );
        assert_eq!(
            synced_target.timeout,
            target.as_ref().unwrap().timeout,
            "{}",
            assert_context_msg::<T>()
        );
        assert_eq!(
            synced_target.endpoint,
            target.as_ref().unwrap().endpoint.to_string(),
            "{}",
            assert_context_msg::<T>()
        );
    }

    tracing::info!("targets_map: {:?}", targets_map);

    let mut synced_executions: Vec<_> = context
        .zitadel_handle
        .list_executions()
        .await
        .with_context(|| format!("{}: Execution not found in zitadel", any::type_name::<T>()))?
        .into_iter()
        .map(|mut execution| {
            execution.targets.iter_mut().for_each(|target| {
                *target = targets_map
                    .get(target)
                    .expect(&format!("Target '{target}' not found in targets_map"))
                    .clone();
            });
            execution
        })
        .collect();

    // Execution with empty targets should be removed
    let mut executions = executions
        .into_iter()
        .filter(|execution| !execution.targets.is_empty())
        .collect::<Vec<_>>();

    executions.sort();
    synced_executions.sort();

    assert_eq!(synced_executions, executions, "{}", assert_context_msg::<T>());

    Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_simple<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = r#"
    target1:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s
    "#;
    let executions = r#"
        - condition: {event: {event: user.human.added}}
          targets: [target1]
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: [target1]
    "#;
    test_v2::<T>(targets, executions, None).await?;

    Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_resync<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = r#"
    target1:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s
    "#;
    let executions = r#"
        - condition: {event: {event: user.human.added}}
          targets: [target1]
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: [target1]
    "#;

    let context = create_context::<T>(None, true).await;
    test_v2::<T>(targets, executions, Some(context.clone())).await?;
    test_v2::<T>(targets, executions, Some(context)).await?;

    Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_update<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = r#"
    target1:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s
    "#;
    let executions = r#"
        - condition: {event: {event: user.human.added}}
          targets: [target1]
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: [target1]
    "#;

    let new_targets = r#"
    target1:
        restWebhook: {interruptOnError: false}
        endpoint: http://example.com/call_me_new
        timeout: 3s
    "#;
    let new_executions = r#"
        - condition: {response: {service: zitadel.session.v2.SessionService}}
          targets: [target1]
        - condition: {request: {method: /zitadel.auth.v1.AuthService/RemoveMyUser}}
          targets: [target1]
    "#;

    let context = create_context::<T>(None, true).await;
    test_v2::<T>(targets, executions, Some(context.clone())).await?;
    test_v2::<T>(new_targets, new_executions, Some(context)).await?;

    Ok(())
}

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_remove<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = r#"
    target1:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s

    target2:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s
    "#;
    let executions = r#"
        - condition: {event: {event: user.human.added}}
          targets: [target1, target2]
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: [target1, target2]
    "#;

    let new_targets = r#"
    target1:
        restAsync: {}
        endpoint: http://example.com/call_me
        timeout: 5s

    target2: null
    "#;
    let new_executions = r#"
        - condition: {event: {event: user.human.added}}
          targets: [target1]
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: []
    "#;

    let context = create_context::<T>(None, true).await;
    test_v2::<T>(targets, executions, Some(context.clone())).await?;
    test_v2::<T>(new_targets, new_executions, Some(context)).await?;

    Ok(())
}

#[allow(clippy::too_many_lines)]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_resync_only_once<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = r#"
    target1:
        restWebhook: {
        }
        endpoint: http://example.com/call_me
        timeout: 5s
    "#;
    let executions = r#"
        - condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
          targets: [target1]
    "#;

    let mock_server = get_zitadel_mock().await;
    let context = create_context::<T>(
        Some(&Url::parse(&mock_server.uri()).expect("Error parsing mock zitadel url")),
        false,
    )
    .await;

    Mock::given(method("POST"))
        .and(path("v2beta/actions/targets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_target_id",
            "signingKey": "test_key"
        })))
        .expect(1)
        .named("create target")
        .mount(&mock_server)
        .await;

    Mock::given(method("DELETE"))
        .and(path_regex(r"v2beta/actions/targets/.*"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .named("delete target")
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("v2beta/actions/executions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .named("set execution")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2beta/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search target - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2beta/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "targets": [
                {
                    "id": "test_target_id",
                    "name": "target1",
                    "restWebhook": {
                        "interruptOnError": false
                    },
                    "timeout": "5s",
                    "endpoint": "http://example.com/call_me",
                    "signingKey": "test_key",
                }
            ]
        })))
        .named("Search target - data")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2beta/actions/executions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search execution - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2beta/actions/executions/search"))
        .respond_with(|req: &wiremock::Request| {
            if req
                .body_json::<V2betaListExecutionsRequest>()
                .unwrap()
                .pagination()
                .unwrap()
                .offset()
                .unwrap_or(&"0".to_owned())
                .parse::<u64>()
                .unwrap()
                > 0
            {
                return ResponseTemplate::new(200).set_body_json(json!({
                    "executions": []
                }));
            }

            ResponseTemplate::new(200).set_body_json(json!({
                "executions": [
                    {
                        "condition": {
                            "request": {
                                "method": "/zitadel.user.v2.UserService/AddHumanUser"
                            }
                        },
                        "targets": ["test_target_id"]
                    }
                ]
            }))
        })
        .named("Search execution - data")
        .mount(&mock_server)
        .await;

    // This path_regex also catches the search target request so it needs to be last
    Mock::given(method("POST"))
        .and(path_regex(r"v2beta/actions/targets/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"signingKey": "test_key"})))
        .expect(0)
        .named("update target")
        .mount(&mock_server)
        .await;

    test_v2::<T>(targets, executions, Some(context.clone())).await?;
    test_v2::<T>(targets, executions, Some(context)).await?;

    Ok(())
}
