use std::{any, marker::PhantomData};

use anyhow::{Context, Result};
#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::Zitadel;
use test_case::test_case;
use tokio::fs;
use tracing_test::traced_test;
#[cfg(feature = "simple-client")]
use zitadel_actions_manager::simple_zitadel_client::SimpleZitadelClient;
use zitadel_actions_manager::{load, sync};

use super::{assert_context_msg, create_context, TestContext, TestZitadelHandle};

async fn test_v1<T: TestZitadelHandle>(
    actions_content: &str,
    flows_content: &str,
    context: Option<TestContext<T>>,
) -> Result<()> {
    let context = context.unwrap_or(create_context::<T>(false).await);
    let actions_path = context.path.path().join("actions.yaml");
    let flows_path = context.path.path().join("flows.yaml");
    fs::write(actions_path.clone(), actions_content).await?;
    fs::write(flows_path.clone(), flows_content).await?;

    let (actions, flows) =
        load(context.path.path(), Some(actions_path.as_path()), Some(flows_path.as_path()))?;

    sync(Some(context.org_id.clone()), &context.zitadel_handle, actions.clone(), flows.clone())
        .await
        .with_context(|| format!("{}: Error syncing the actions", any::type_name::<T>()))?;

    for (action_name, action) in actions.iter() {
        let synced_action = context
            .zitadel_handle
            .search_actions_by_name(action_name, Some(context.org_id.clone()))
            .await?
            .with_context(|| {
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
            synced_action.allowed_to_fail.unwrap_or_default(),
            action.as_ref().unwrap().allowed_to_fail.unwrap_or_default(),
            "{}",
            assert_context_msg::<T>()
        );
    }

    for (flow_name, flow) in flows.iter() {
        let synced_flow =
            context.zitadel_handle.get_triggers(flow_name, Some(context.org_id.clone())).await?;
        for (trigger_type, trigger_actions) in flow.iter() {
            let synced_trigger_actions = synced_flow
                .iter()
                .find(|t| &t.trigger_type.id == trigger_type)
                .map(|t| t.actions.iter().map(|a| a.name.clone()).collect::<Vec<_>>())
                .with_context(|| {
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

    let context = create_context::<T>(false).await;
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

    let context = create_context::<T>(false).await;
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

    let context = create_context::<T>(false).await;
    test_v1::<T>(actions, flows, Some(context.clone())).await?;
    test_v1::<T>(new_actions, new_flows, Some(context)).await?;

    Ok(())
}
