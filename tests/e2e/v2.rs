// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

use std::{any, collections::HashMap, marker::PhantomData};

#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::Zitadel;
use famedly_zitadel_rust_client::v2::actions::V2ListExecutionsRequest;
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
use zitadel_actions_manager::{
    v2,
    zitadel::{PayloadType, decode_public_key_pem, encode_public_key_pem, normalize_pem},
};

use super::{
    Result, TestContext, TestZitadelHandle, assert_context_msg, create_context, eventually,
};
use crate::e2e::get_zitadel_mock;

#[allow(clippy::too_many_lines)]
async fn test_v2<T: TestZitadelHandle>(
    targets_content: &str,
    executions_content: &str,
    context: Option<TestContext<T>>,
) -> Result<()> {
    let context = context.unwrap_or(create_context::<T>(None, true).await);
    let targets_path = context.path.path().join("targets.yaml");
    let executions_path = context.path.path().join("executions.yaml");
    fs::write(targets_path.clone(), targets_content)
        .await
        .whatever_context("Error writing targets")?;
    fs::write(executions_path.clone(), executions_content)
        .await
        .whatever_context("Error writing executions")?;

    let (targets, executions) = v2::load(
        context.path.path(),
        Some(targets_path.as_path()),
        Some(executions_path.as_path()),
    )
    .whatever_context("Error loading targets and executions")?;

    v2::sync(&context.zitadel_handle, targets.clone(), executions.clone())
        .await
        .whatever_context("Error syncing the targets")?;

    eventually(|| async {
        let mut targets_map = HashMap::new();

        for (target_name, target) in targets.iter() {
            let synced_target = context
                .zitadel_handle
                .search_target_by_name(target_name)
                .await
                .whatever_context("Error searching target by name")?
                .with_whatever_context(|| {
                    format!(
                        "{}: Target '{target_name}' not found in zitadel",
                        any::type_name::<T>()
                    )
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
            assert!(
                PayloadType::eq_optional(
                    target.as_ref().unwrap().effective_payload_type(),
                    synced_target.payload_type,
                ),
                "{}: payload type mismatch for '{target_name}': local={:?} remote={:?}",
                assert_context_msg::<T>(),
                target.as_ref().unwrap().effective_payload_type(),
                synced_target.payload_type,
            );
        }

        tracing::info!("targets_map: {:?}", targets_map);

        let mut synced_executions: Vec<_> = context
            .zitadel_handle
            .list_executions()
            .await
            .with_whatever_context(|_| {
                format!("{}: Execution not found in zitadel", any::type_name::<T>())
            })?
            .into_iter()
            // .filter(|execution| !execution.targets.is_empty())
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
            .iter()
            .filter(|execution| !execution.targets.is_empty())
            .cloned()
            .collect::<Vec<_>>();

        executions.sort();
        synced_executions.sort();

        assert_eq!(synced_executions, executions, "{}", assert_context_msg::<T>());

        Ok(())
    })
    .await
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
        restWebhook:
            interruptOnError: true
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
        .and(path("v2/actions/targets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "test_target_id",
            "signingKey": "test_key"
        })))
        .expect(1)
        .named("create target")
        .mount(&mock_server)
        .await;

    Mock::given(method("DELETE"))
        .and(path_regex(r"v2/actions/targets/.*"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .named("delete target")
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("v2/actions/executions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .named("set execution")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search target - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "targets": [
                {
                    "id": "test_target_id",
                    "name": "target1",
                    "restWebhook": {
                        "interruptOnError": true
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
        .and(path("v2/actions/executions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search execution - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/executions/search"))
        .respond_with(|req: &wiremock::Request| {
            if req
                .body_json::<V2ListExecutionsRequest>()
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

    // This path_regex also catches the search target request so it needs to be
    // last
    Mock::given(method("POST"))
        .and(path_regex(r"v2/actions/targets/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"signingKey": "test_key"})))
        .expect(0)
        .named("update target")
        .mount(&mock_server)
        .await;

    test_v2::<T>(targets, executions, Some(context.clone())).await?;
    test_v2::<T>(targets, executions, Some(context)).await?;

    Ok(())
}

const TEST_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAoAL7geWB02K3b90hAV9M
QIatE+L0Hj6b1uuFf4iOGkgVeqnxfnwqtZsEU3xxUxGhMZ2Qjr7xrODMBxnI+tos
DuoeGVmCqYg+szRtca4XhxakoFMZV9JnZDH862qvqKICHhZj796OUwCM00MfozCE
I5/kWKGWoP39koVoaB8UKDrrSUvB8yH1lbB8F4tOBzED9K4BRle4u1WW/Dl74Y4a
gysWRYrxVxbQN3AFJq5V+93EpEFhIxZT4PIuL+v2yLxHOB/x9N1FKxxq4AxDROMv
OOM2mE9Dzif/JR/30BFs6aZL0ciASQva4X9Fskdlal3qk5Kn2pNNXYXk0FQPS50t
TwIDAQAB
-----END PUBLIC KEY-----
";

fn payload_types_targets_yaml(public_key_pem: &str) -> String {
    let indented_pem =
        public_key_pem.lines().map(|line| format!("      {line}")).collect::<Vec<_>>().join("\n");
    format!(
        r#"
json_target:
    restCall:
        interruptOnError: true
    endpoint: http://example.com/json
    timeout: 5s
    payloadType: PAYLOAD_TYPE_JSON

jwt_target:
    restAsync: {{}}
    endpoint: http://example.com/jwt
    timeout: 5s
    payloadType: PAYLOAD_TYPE_JWT

jwe_target:
    restAsync: {{}}
    endpoint: http://example.com/jwe
    timeout: 5s
    payloadType: PAYLOAD_TYPE_JWE
    publicKey: |
{indented_pem}
"#
    )
}

const PAYLOAD_TYPES_EXECUTIONS: &str = r#"
- condition: {event: {event: user.human.added}}
  targets: [json_target]
- condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
  targets: [jwt_target]
- condition: {response: {service: zitadel.session.v2.SessionService}}
  targets: [jwe_target]
"#;

#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_payload_types<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = payload_types_targets_yaml(TEST_PUBLIC_KEY_PEM);
    let context = create_context::<T>(None, true).await;
    test_v2::<T>(&targets, PAYLOAD_TYPES_EXECUTIONS, Some(context.clone())).await?;

    eventually(|| async {
        let jwe = context
            .zitadel_handle
            .search_target_by_name("jwe_target")
            .await
            .whatever_context("Error searching jwe_target")?
            .whatever_context("jwe_target missing")?;
        assert_eq!(jwe.payload_type, Some(PayloadType::Jwe), "{}", assert_context_msg::<T>());

        let keys = context
            .zitadel_handle
            .list_public_keys(&jwe.id)
            .await
            .whatever_context("Error listing public keys")?;
        let active: Vec<_> = keys.iter().filter(|key| key.active).collect();
        assert_eq!(active.len(), 1, "{}", assert_context_msg::<T>());
        let remote_pem = decode_public_key_pem(
            active[0].public_key.as_deref().whatever_context("public key missing")?,
        )
        .whatever_context("Failed to decode public key")?;
        assert_eq!(
            normalize_pem(&remote_pem),
            normalize_pem(TEST_PUBLIC_KEY_PEM),
            "{}",
            assert_context_msg::<T>()
        );
        Ok(())
    })
    .await?;

    test_v2::<T>(&targets, PAYLOAD_TYPES_EXECUTIONS, Some(context.clone())).await?;

    // Reset jwt_target to implicit JSON default (omit payloadType in YAML).
    let targets_reset_jwt = payload_types_targets_yaml(TEST_PUBLIC_KEY_PEM)
        .replace("    payloadType: PAYLOAD_TYPE_JWT\n", "");
    test_v2::<T>(&targets_reset_jwt, PAYLOAD_TYPES_EXECUTIONS, Some(context.clone())).await?;

    eventually(|| async {
        let jwt = context
            .zitadel_handle
            .search_target_by_name("jwt_target")
            .await
            .whatever_context("Error searching jwt_target")?
            .whatever_context("jwt_target missing")?;
        assert!(
            PayloadType::eq_optional(None, jwt.payload_type),
            "{}: jwt_target should reset to JSON default, got {:?}",
            assert_context_msg::<T>(),
            jwt.payload_type,
        );
        Ok(())
    })
    .await?;

    // Second sync with omitted payloadType must be a no-op (idempotent).
    test_v2::<T>(&targets_reset_jwt, PAYLOAD_TYPES_EXECUTIONS, Some(context)).await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_jwe_public_key_sync_once<T: TestZitadelHandle>(_: PhantomData<T>) -> Result<()> {
    let targets = format!(
        r#"
jwe_target:
    restCall:
        interruptOnError: true
    endpoint: http://example.com/jwe
    timeout: 5s
    payloadType: PAYLOAD_TYPE_JWE
    publicKey: |
{}"#,
        TEST_PUBLIC_KEY_PEM
            .lines()
            .map(|line| format!("      {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let executions = r#"
- condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
  targets: [jwe_target]
"#;

    let mock_server = get_zitadel_mock().await;
    let context = create_context::<T>(
        Some(&Url::parse(&mock_server.uri()).expect("Error parsing mock zitadel url")),
        false,
    )
    .await;

    let encoded_key = encode_public_key_pem(TEST_PUBLIC_KEY_PEM);

    Mock::given(method("POST"))
        .and(path("v2/actions/targets"))
        .and(wiremock::matchers::body_partial_json(json!({
            "payloadType": "PAYLOAD_TYPE_JWE",
            "restCall": { "interruptOnError": true }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "jwe_target_id",
            "signingKey": "test_key"
        })))
        .expect(1)
        .named("create jwe target")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search target - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "targets": [{
                "id": "jwe_target_id",
                "name": "jwe_target",
                "restCall": { "interruptOnError": true },
                "timeout": "5s",
                "endpoint": "http://example.com/jwe",
                "signingKey": "test_key",
                "payloadType": "PAYLOAD_TYPE_JWE"
            }]
        })))
        .named("Search target - data")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("list public keys - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keyId": "pub_key_1"
        })))
        .expect(1)
        .named("add public key")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys/pub_key_1/activate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .named("activate public key")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "publicKeys": [{
                "keyId": "pub_key_1",
                "active": true,
                "publicKey": encoded_key
            }]
        })))
        .named("list public keys - data")
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("v2/actions/executions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .named("set execution")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/executions/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .up_to_n_times(1)
        .named("Search execution - empty")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/executions/search"))
        .respond_with(|req: &wiremock::Request| {
            if req
                .body_json::<V2ListExecutionsRequest>()
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
                "executions": [{
                    "condition": {
                        "request": {
                            "method": "/zitadel.user.v2.UserService/AddHumanUser"
                        }
                    },
                    "targets": ["jwe_target_id"]
                }]
            }))
        })
        .named("Search execution - data")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"v2/actions/targets/[^/]+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .named("update target")
        .mount(&mock_server)
        .await;

    test_v2::<T>(&targets, executions, Some(context.clone())).await?;
    test_v2::<T>(&targets, executions, Some(context)).await?;

    Ok(())
}

/// Matching PEM with a different expiration must upload a new key (expiration
/// is part of desired state; Zitadel cannot update expiration in place).
#[allow(clippy::too_many_lines)]
#[cfg_attr(feature = "famedly-zitadel-rust-client",test_case(PhantomData::<Zitadel>; "zrc"))]
#[cfg_attr(feature = "simple-client",test_case(PhantomData::<SimpleZitadelClient>; "szc"))]
#[tokio::test]
#[traced_test]
async fn test_jwe_public_key_expiration_change_uploads_new_key<T: TestZitadelHandle>(
    _: PhantomData<T>,
) -> Result<()> {
    const NEW_EXPIRATION: &str = "2027-01-01T00:00:00Z";
    let targets = format!(
        r#"
jwe_target:
    restCall:
        interruptOnError: true
    endpoint: http://example.com/jwe
    timeout: 5s
    payloadType: PAYLOAD_TYPE_JWE
    publicKeyExpiration: "{NEW_EXPIRATION}"
    publicKey: |
{}"#,
        TEST_PUBLIC_KEY_PEM
            .lines()
            .map(|line| format!("      {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let executions = r#"
- condition: {request: {method: /zitadel.user.v2.UserService/AddHumanUser}}
  targets: [jwe_target]
"#;

    let mock_server = get_zitadel_mock().await;
    let context = create_context::<T>(
        Some(&Url::parse(&mock_server.uri()).expect("Error parsing mock zitadel url")),
        false,
    )
    .await;

    let encoded_key = encode_public_key_pem(TEST_PUBLIC_KEY_PEM);

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "targets": [{
                "id": "jwe_target_id",
                "name": "jwe_target",
                "restCall": { "interruptOnError": true },
                "timeout": "5s",
                "endpoint": "http://example.com/jwe",
                "signingKey": "test_key",
                "payloadType": "PAYLOAD_TYPE_JWE"
            }]
        })))
        .named("Search target - existing")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "publicKeys": [{
                "keyId": "pub_key_old",
                "active": true,
                "publicKey": encoded_key,
                "expirationDate": "2026-01-01T00:00:00Z"
            }]
        })))
        .named("list public keys - old expiration")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys"))
        .and(wiremock::matchers::body_partial_json(json!({
            "expirationDate": NEW_EXPIRATION
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keyId": "pub_key_new"
        })))
        .expect(1)
        .named("add public key with new expiration")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/targets/jwe_target_id/publickeys/pub_key_new/activate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .named("activate new public key")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("v2/actions/executions/search"))
        .respond_with(|req: &wiremock::Request| {
            if req
                .body_json::<V2ListExecutionsRequest>()
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
                "executions": [{
                    "condition": {
                        "request": {
                            "method": "/zitadel.user.v2.UserService/AddHumanUser"
                        }
                    },
                    "targets": ["jwe_target_id"]
                }]
            }))
        })
        .named("Search execution - unchanged")
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path_regex(r"v2/actions/targets/[^/]+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .named("update target")
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("v2/actions/executions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .named("set execution")
        .mount(&mock_server)
        .await;

    test_v2::<T>(&targets, executions, Some(context)).await?;

    Ok(())
}
