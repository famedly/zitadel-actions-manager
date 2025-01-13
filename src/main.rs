#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Parser;
use famedly_rust_utils::{BaseUrl, LevelFilter};
use serde::de::DeserializeOwned;
use tracing::{info, instrument};
use zitadel_actions_manager::{
    load_actions,
    simple_zitadel_client::{auth_with_service_account, ServiceAccount, SimpleZitadelClient},
    sync, Actions, Traced,
};

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"), ", git rev ", env!("VERGEN_GIT_SHA"));

type BoxedErr = Box<dyn std::error::Error>;

#[derive(Parser, Debug)]
#[command(about, version = VERSION)]
/// A tool to sync/migrate Zitadel actions defined in a declarative way.
struct Args {
    /// File to read actions from
    #[arg(short, long, default_value = "actions.yaml", value_name = "PATH")]
    actions: String,

    /// File to read flows from
    #[arg(short, long, default_value = "flows.yaml", value_name = "PATH")]
    flows: String,

    /// Directory with actions
    #[arg(short, long, default_value = ".")]
    dir: String,

    /// Zitadel Url
    #[arg(short, long, default_value = "http://localhost:9310")]
    url: BaseUrl,

    /// Zitadel access token
    #[arg(short, long, env = "ZITADEL_JWT", hide_env_values = true)]
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

#[tokio::test]
async fn test_binary() -> Result<(), Traced<BoxedErr>> {
    init_tracing(&tracing_subscriber::filter::LevelFilter::TRACE, None);
    run(Args::parse_from([
        "binname",
        "-d=example-actions",
        "-s=/tmp/zitadel-docker-test/service-account.json",
        "--aud=http://localhost:9310",
        "-u=http://localhost:9310",
    ]))
    .await
    .inspect_err(|e| tracing::error!("{e}"))
}

#[instrument(level = "error")]
fn from_yaml_file<T: DeserializeOwned, P: std::fmt::Debug + AsRef<Path>>(
    path: P,
) -> Result<T, Traced<BoxedErr>> {
    Ok(serde_yaml::from_reader(File::open(path).map_err(BoxedErr::from)?)
        .map_err(BoxedErr::from)?)
}

async fn run(args: Args) -> Result<(), Traced<BoxedErr>> {
    let flows_fname = [args.dir.as_str(), &args.flows].join("/");
    let actions_fname = [args.dir.as_str(), &args.actions].join("/");
    let flows = from_yaml_file(&flows_fname)?;

    let actions = if std::fs::exists(&actions_fname).map_err(BoxedErr::from)? {
        from_yaml_file(&actions_fname)?
    } else {
        info!("File {actions_fname:?} doesn't exist, reading only actions referenced in {flows_fname:?}");
        Actions::default()
    };

    let access_token = if let Some(svc_acc_file) = args.service_account {
        let aud = args.aud.ok_or_else(|| {
            boxed_text_err("--aud must be specified along with --service-account")
        })?;
        let service_account: ServiceAccount = from_yaml_file(&svc_acc_file)?;
        auth_with_service_account(&args.url, &aud, &service_account)
            .await
            .map_err(Traced::map_from)?
    } else {
        args.token.ok_or_else(|| {
            boxed_text_err("Either --token or --service-account must be specified")
        })?
    };

    info!("Loading all actions...");
    let loaded_actions = load_actions(&args.dir, actions, &flows).map_err(Traced::map_from)?;
    let zitadel = SimpleZitadelClient::new(args.url.clone(), &access_token, args.org_id.clone())
        .map_err(|e| Traced::new(BoxedErr::from(e)))?;

    info!("Performing sync...");

    if args.all_orgs {
        const PAGE_SIZE: u64 = 100;
        let mut page = 0;
        // Using `while let` here results in `future not Send`
        // FIXME: investigate, possibly compiler bug
        loop {
            let Some(org_ids) = zitadel
                .get_all_orgs(page * PAGE_SIZE, PAGE_SIZE)
                .await
                .map_err(Traced::map_from)?
            else {
                break;
            };
            for org_id in org_ids {
                let zitadel =
                    SimpleZitadelClient::new(args.url.clone(), &access_token, Some(org_id.clone()))
                        .map_err(|e| Traced::new(BoxedErr::from(e)))?;
                sync(false, Some(org_id), &zitadel, loaded_actions.clone(), flows.clone())
                    .await
                    .map_err(Traced::map_from)?;
            }
            page += 1;
        }
    } else {
        sync(false, args.org_id, &zitadel, loaded_actions, flows)
            .await
            .map_err(Traced::map_from)?;
    }
    Ok(())
}

fn boxed_text_err(e: &str) -> Traced<BoxedErr> {
    Traced::new(BoxedErr::from(std::io::Error::other(e)))
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
