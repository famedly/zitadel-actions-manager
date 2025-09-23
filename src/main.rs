// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

#![allow(missing_docs, clippy::missing_docs_in_private_items)]

use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use famedly_rust_utils::{BaseUrl, LevelFilter};
use snafu::{OptionExt as _, Snafu};
use tracing::info;
use zitadel_actions_manager::{
    from_yaml_file, instrument, load,
    simple_zitadel_client::{
        auth_with_service_account, ServiceAccount, SimpleZitadelClient,
        SimpleZitadelClientCreationError, SimpleZitadelClientError,
    },
    sync,
    v2::{self, DEFAULT_EXECUTIONS_FILE, DEFAULT_TARGETS_FILE},
    Actions, Flows, LoadActionsV1Error, LoadedScript, ReadYamlFileError, SpanTraceWrapper,
    DEFAULT_ACTIONS_FILE, DEFAULT_FLOWS_FILE,
};

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"), ", git rev ", env!("VERGEN_GIT_SHA"));

#[derive(Debug, Snafu)]
#[snafu(whatever, display("{message}"))]
struct CliError {
    message: String,
    #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
    source: Option<Box<dyn std::error::Error>>,
    #[snafu(implicit)]
    context: SpanTraceWrapper,
}

#[derive(Parser, Debug)]
#[command(about, version = VERSION)]
/// A tool to sync/migrate Zitadel actions defined in a declarative way.
struct Args {
    /// Run v1 actions sync
    #[arg(short = '1', long)]
    v1: bool,

    /// Run v2 actions sync
    #[arg(short = '2', long)]
    v2: bool,

    /// File to read actions from (v1)
    #[arg(short, long, default_value = DEFAULT_ACTIONS_FILE, value_name = "PATH")]
    actions: String,

    /// File to read flows from (v1)
    #[arg(short, long, default_value = DEFAULT_FLOWS_FILE, value_name = "PATH")]
    flows: String,

    /// File to read targets from (v2)
    #[arg(short, long, default_value = DEFAULT_TARGETS_FILE, value_name = "PATH")]
    targets: String,

    /// File to read executions from (v2)
    #[arg(short, long, default_value = DEFAULT_EXECUTIONS_FILE, value_name = "PATH")]
    executions: String,

    /// Directory with actions
    #[arg(short, long, default_value = ".")]
    dir: String,

    /// Zitadel Url
    #[arg(short, long, default_value = "http://localhost:9310")]
    url: BaseUrl,

    /// Zitadel access token
    #[arg(short = 'T', long, env = "ZITADEL_JWT", hide_env_values = true)]
    token: Option<String>,

    /// Zitadel service account file
    #[arg(short, long, value_name = "PATH")]
    service_account: Option<PathBuf>,

    /// Audience to add to zitadel JWT (used with `--service-account`)
    #[arg(long)]
    aud: Option<String>,

    /// Organization for which perform the sync
    #[arg(short, long)]
    org_id: Option<String>,

    /// Sync for all orgs
    #[arg(short = 'A', long, default_value_t = false)]
    all_orgs: bool,

    /// Log level <off|trace|debug|warn|error>
    #[arg(short, long, env = "LOG_LEVEL", default_value = "info")]
    log_level: LevelFilter,
}

#[allow(clippy::print_stdout)]
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    init_tracing(&args.log_level, None);
    println!("{} {VERSION}", env!("CARGO_PKG_NAME"));

    match run(args).await.inspect_err(|e| tracing::error!("{}", e)) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[instrument(skip_all)]
async fn run(args: Args) -> Result<(), CliError> {
    (args.v1 || args.v2)
        .then_some(())
        .whatever_context::<_, CliError>("Either --v1 or --v2 should be specified")?;

    let actions_and_flows = args
        .v1
        .then(|| {
            info!("Loading all v1 actions and flows...");
            load(args.dir.as_ref(), Some(args.actions.as_ref()), Some(args.flows.as_ref()))
        })
        .transpose()?;

    let targets_and_executions = args
        .v2
        .then(|| {
            info!("Loading all v2 targets and executions...");
            v2::load(args.dir.as_ref(), Some(args.targets.as_ref()), Some(args.executions.as_ref()))
        })
        .transpose()?;

    let access_token = auth(&args).await?;

    let zitadel = SimpleZitadelClient::new(args.url.clone(), &access_token, args.org_id.clone())?;

    if let Some((loaded_actions, flows)) = actions_and_flows {
        info!("Performing v1 actions sync...");
        sync_v1(&args, &zitadel, loaded_actions, flows).await?;
    }

    if let Some((targets, executions)) = targets_and_executions {
        info!("Performing v2 actions sync...");
        v2::sync(&zitadel, targets, executions).await?;
    }

    Ok(())
}

#[instrument(skip_all)]
async fn sync_v1(
    args: &Args,
    zitadel: &SimpleZitadelClient,
    actions: Actions<LoadedScript>,
    flows: Flows,
) -> Result<(), CliError> {
    if args.all_orgs {
        const PAGE_SIZE: u64 = 100;
        let mut page = 0;
        // TODO: refactor to `while let` if error type becomes `Send`
        loop {
            let Some(org_ids) = zitadel.get_all_orgs(page * PAGE_SIZE, PAGE_SIZE).await? else {
                break;
            };

            for org_id in org_ids {
                sync(Some(org_id), zitadel, actions.clone(), flows.clone()).await?;
            }
            page += 1;
        }
    } else {
        sync(args.org_id.clone(), zitadel, actions, flows).await?;
    }
    Ok(())
}

#[instrument(skip_all)]
async fn auth(args: &Args) -> Result<String, CliError> {
    if let Some(svc_acc_file) = &args.service_account {
        let aud = args.aud.as_ref().whatever_context::<_, CliError>(
            "--aud must be specified along with --service-account",
        )?;
        let service_account: ServiceAccount = from_yaml_file(svc_acc_file)?;
        Ok(auth_with_service_account(&args.url, aud, &service_account).await?)
    } else {
        args.token.clone().whatever_context("Either --token or --service-account must be specified")
    }
}

#[allow(clippy::print_stdout, clippy::expect_used)]
pub fn init_tracing(
    level: &tracing_subscriber::filter::LevelFilter,
    additional_env_filters: Option<String>,
) {
    use std::str::FromStr;

    use tracing::Level;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let raw_env_filter = format!(
        "info,{}={level}{}",
        env!("CARGO_CRATE_NAME"),
        additional_env_filters.map_or("".into(), |s| [",", &s].concat())
    );
    println!("Tracing filter: {raw_env_filter:?}");
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::from_str(&raw_env_filter))
        .expect("Invalid tracing env filter");

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(level >= &Level::DEBUG)
                .with_line_number(level >= &Level::DEBUG),
        )
        .with(tracing_error::ErrorLayer::default())
        .try_init()
        .expect("Failed to initialize tracing subscriber");
}

impl From<LoadActionsV1Error> for CliError {
    fn from(error: LoadActionsV1Error) -> Self {
        CliError {
            message: error.to_string(),
            context: error.get_context().clone(),
            source: Some(Box::new(error)),
        }
    }
}

impl From<ReadYamlFileError> for CliError {
    fn from(error: ReadYamlFileError) -> Self {
        CliError {
            message: error.to_string(),
            context: error.get_context().clone(),
            source: Some(Box::new(error)),
        }
    }
}

impl From<SimpleZitadelClientCreationError> for CliError {
    fn from(error: SimpleZitadelClientCreationError) -> Self {
        CliError {
            message: error.to_string(),
            context: error.get_context().clone(),
            source: Some(Box::new(error)),
        }
    }
}

impl From<SimpleZitadelClientError> for CliError {
    fn from(error: SimpleZitadelClientError) -> Self {
        CliError {
            message: error.to_string(),
            context: error.get_context().clone(),
            source: Some(Box::new(error)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[tokio::test]
    async fn test_e2e_binary_v1() -> Result<(), CliError> {
        init_tracing(&tracing_subscriber::filter::LevelFilter::TRACE, None);
        run(Args::parse_from([
            "binname",
            "--v1",
            "-d=example-actions",
            "-s=docker/zitadel/service-account.json",
            "--aud=http://localhost:9310",
            "-u=http://localhost:9310",
        ]))
        .await
        .inspect_err(|e| tracing::error!("{e}"))
    }

    #[tokio::test]
    async fn test_e2e_binary_v2() -> Result<(), CliError> {
        init_tracing(&tracing_subscriber::filter::LevelFilter::TRACE, None);
        run(Args::parse_from([
            "binname",
            "--v2",
            "-d=example-actions",
            "-s=docker/zitadel/service-account.json",
            "--aud=http://localhost:9310",
            "-u=http://localhost:9310",
        ]))
        .await
        .inspect_err(|e| tracing::error!("{e}"))
    }
}
