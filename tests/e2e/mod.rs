// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

use std::{
    any,
    future::Future,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

#[cfg(feature = "simple-client")]
use famedly_zitadel_rust_client::v2::authentication::Token;
#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::{organization::V2AddOrganizationRequest, Zitadel};
#[cfg(any(feature = "simple-client", feature = "famedly-zitadel-rust-client"))]
use reqwest_middleware::ClientBuilder;
use serde_json::json;
use snafu::{OptionExt as _, ResultExt as _};
use url::Url;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};
#[cfg(feature = "simple-client")]
use zitadel_actions_manager::simple_zitadel_client::SimpleZitadelClient;
use zitadel_actions_manager::zitadel::{ZitadelHandle, ZitadelHandleV2};

mod v1;
mod v2;

type Result<T, E = snafu::Whatever> = std::result::Result<T, E>;

static ZITADEL_URL: LazyLock<Url> =
    LazyLock::new(|| Url::parse("http://localhost:9310").expect("Error parsing zitadel url"));

static ZITADEL_SERVICE_USER_PATH: &str = "docker/zitadel/service-account.json";

struct TestContext<T: ZitadelHandle + ZitadelHandleV2 + Clone> {
    pub path: tempfile::TempDir,
    pub zitadel_handle: T,
    pub org_id: String,
}

impl<T: ZitadelHandle + ZitadelHandleV2 + Clone> Clone for TestContext<T> {
    fn clone(&self) -> Self {
        Self {
            path: tempfile::tempdir().expect("Failed to create temp dir"),
            zitadel_handle: self.zitadel_handle.clone(),
            org_id: self.org_id.clone(),
        }
    }
}

trait TestZitadelHandle: ZitadelHandle + ZitadelHandleV2 + Clone {
    async fn new(url: Url, path: PathBuf) -> Self;
    async fn create_org(&self) -> Result<String>;
    async fn list_targets_id(&self) -> Result<Vec<String>>;
}

#[cfg(feature = "simple-client")]
impl TestZitadelHandle for SimpleZitadelClient {
    async fn new(url: Url, path: PathBuf) -> Self {
        let token = Token::new(
            url.clone(),
            &path,
            ClientBuilder::new(reqwest::Client::new()).build(),
            None,
            None,
        )
        .await
        .expect("Error creating zitadel token");
        SimpleZitadelClient::new(
            url.try_into().expect("Url is not a base url"),
            &token.token().await.expect("Error getting token"),
            None,
        )
        .expect("Error creating zitadel simple client")
    }
    async fn create_org(&self) -> Result<String> {
        self.create_org(&generate_random_string::<10>())
            .await
            .whatever_context("Error creating organization")
    }
    async fn list_targets_id(&self) -> Result<Vec<String>> {
        self.list_targets_id().await.whatever_context("Error listing targets")
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl TestZitadelHandle for Zitadel {
    async fn new(url: Url, path: PathBuf) -> Self {
        let client = ClientBuilder::new(reqwest::Client::new()).build();
        Zitadel::new(client, url, path, None).await.expect("Error creating zitadel client")
    }
    async fn create_org(&self) -> Result<String> {
        self.create_organization_with_admin(V2AddOrganizationRequest::new(
            generate_random_string::<10>(),
        ))
        .await
        .whatever_context("Error creating organization")?
        .organization_id()
        .cloned()
        .whatever_context("Created organization is missing id")
    }
    async fn list_targets_id(&self) -> Result<Vec<String>> {
        use futures::{StreamExt, TryStreamExt};
        use snafu::{FromString as _, Whatever};

        self.list_targets(&None, &None, &None)
            .map_err(|e| Whatever::with_source(e.into(), "Error listing targets".to_owned()))
            .map(|target| {
                target.and_then(|target| {
                    target.id().cloned().whatever_context("Target is missing id")
                })
            })
            .try_collect::<Vec<String>>()
            .await
    }
}

async fn create_context<T: TestZitadelHandle + ZitadelHandle + ZitadelHandleV2 + Clone>(
    zitadel_url: Option<&Url>,
    clean_v2_actions: bool,
) -> TestContext<T> {
    let path = tempfile::tempdir().expect("Failed to create temp dir");

    let zitadel_handle = T::new(
        zitadel_url.unwrap_or(&ZITADEL_URL).clone(),
        Path::new(ZITADEL_SERVICE_USER_PATH).to_path_buf(),
    )
    .await;
    let org_id = zitadel_handle.create_org().await.expect("Error creating organization");

    if clean_v2_actions {
        clean_up_v2_actions(&zitadel_handle).await;
    }

    TestContext { path, zitadel_handle, org_id }
}

#[must_use]
pub fn generate_random_string<const N: usize>() -> String {
    let mut buf: [u8; N] = [0; N];
    getrandom::fill(&mut buf).unwrap();
    buf.into_iter().map(|x| char::from_u32(0x61 + u32::from(x) % 26).unwrap()).collect()
}

pub fn assert_context_msg<T>() -> String {
    any::type_name::<T>().to_owned()
}

async fn clean_up_v2_actions<T: TestZitadelHandle + ZitadelHandleV2>(zitadel: &T) {
    // Delete and wait until the search projection catches up; otherwise the next
    // sync may reuse deleted target IDs and fail with "Target not found".
    for _ in 0..30 {
        let targets_id = zitadel.list_targets_id().await.expect("Error listing targets");
        if targets_id.is_empty() {
            return;
        }
        for target_id in targets_id {
            zitadel.delete_target(&target_id).await.expect("Error deleting target");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for v2 targets cleanup");
}

/// Retry `f` while Zitadel projections catch up after writes.
///
/// Retries on both `Err` and panics from `assert!` / `assert_eq!`.
pub async fn eventually<F, Fut, E>(mut f: F) -> Result<(), E>
where
    F: FnMut() -> Fut + Send,
    Fut: Future<Output = Result<(), E>> + Send,
{
    use std::panic::AssertUnwindSafe;

    use futures::FutureExt as _;
    let attempts = 30;

    for attempt in 1..=attempts {
        match AssertUnwindSafe(f()).catch_unwind().await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(err)) => {
                if attempt == attempts {
                    return Err(err);
                }
            }
            Err(panic) => {
                if attempt == attempts {
                    std::panic::resume_unwind(panic);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    unreachable!("loop always returns on the final attempt")
}

pub async fn get_zitadel_mock() -> MockServer {
    // Start a background HTTP server on a random local port
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/v2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "test",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        // Mounting the mock on the mock server - it's now effective!
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v2/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "organizationId": "test_org_id",
        })))
        // Mounting the mock on the mock server - it's now effective!
        .mount(&mock_server)
        .await;

    mock_server
}
