use std::{
    any,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, Result};
#[cfg(feature = "famedly-zitadel-rust-client")]
use famedly_zitadel_rust_client::v2::{
    authentication::Token, organization::V2AddOrganizationRequest, Zitadel,
};
use url::Url;
#[cfg(feature = "simple-client")]
use zitadel_actions_manager::simple_zitadel_client::SimpleZitadelClient;
use zitadel_actions_manager::zitadel::{ZitadelHandle, ZitadelHandleV2};

mod v1;
mod v2;

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
        let token = Token::new(url.clone(), &path, reqwest::Client::new(), None, None)
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
        self.create_org(&generate_random_string::<10>()).await
    }
    async fn list_targets_id(&self) -> Result<Vec<String>> {
        self.list_targets_id().await
    }
}

#[cfg(feature = "famedly-zitadel-rust-client")]
impl TestZitadelHandle for Zitadel {
    async fn new(url: Url, path: PathBuf) -> Self {
        Zitadel::new(url, path, None).await.expect("Error creating zitadel client")
    }
    async fn create_org(&self) -> Result<String> {
        self.create_organization_with_admin(V2AddOrganizationRequest::new(
            generate_random_string::<10>(),
        ))
        .await?
        .organization_id()
        .cloned()
        .context("Created organization is missing id")
    }
    async fn list_targets_id(&self) -> Result<Vec<String>> {
        use futures::{StreamExt, TryStreamExt};

        self.list_targets(&None, &None, &None)
            .map(|taregt| {
                taregt.and_then(|target| target.id().cloned().context("Target is missing id"))
            })
            .try_collect::<Vec<String>>()
            .await
    }
}

async fn create_context<T: TestZitadelHandle + ZitadelHandle + ZitadelHandleV2 + Clone>(
    clean_v2_actions: bool,
) -> TestContext<T> {
    let path = tempfile::tempdir().expect("Failed to create temp dir");

    let zitadel_handle =
        T::new(ZITADEL_URL.clone(), Path::new(ZITADEL_SERVICE_USER_PATH).to_path_buf()).await;
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
    let targets_id = zitadel.list_targets_id().await.expect("Error listing targets");
    for target_id in targets_id {
        zitadel.delete_target(&target_id).await.expect("Error deleting target");
    }
}
