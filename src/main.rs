#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use std::{fs::File, process::ExitCode};

use clap::Parser;
use famedly_rust_utils::{BaseUrl, LevelFilter};
use serde_yaml::from_reader as from_yaml_file;
use tracing::info;
use zitadel_actions_sync::{
    load_actions, simple_zitadel_client::SimpleZitadelClient, sync, Actions,
};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// File to read actions from
    #[arg(short, long, default_value = "actions.yaml")]
    actions: String,

    /// File to read flows from
    #[arg(short, long, default_value = "flows.yaml")]
    flows: String,

    /// Directory with actions
    #[arg(short, long, default_value = ".")]
    dir: String,

    /// Zitadel Url
    #[arg(short, long, default_value = "http://localhost:9310")]
    url: BaseUrl,

    /// Zitadel access token
    #[arg(short, long, env = "ZITADEL_JWT")]
    token: String,

    /// Organization for which perform the sync
    #[arg(short, long)]
    org_id: Option<String>,

    /// Log level <off|trace|debug|warn|error>
    #[arg(short, long, env = "LOG_LEVEL", default_value = "info")]
    log_level: LevelFilter,
}

#[allow(clippy::print_stdout)]
#[tokio::main]
async fn main() -> ExitCode {
    println!(
        "{} v{}, git rev {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA")
    );

    let args = Args::parse();
    init_tracing(&args.log_level, Some("reqwest=info".into()));

    match run(args).await.inspect_err(|e| tracing::error!("{}", e)) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let flows_fname = [args.dir.as_str(), &args.flows].join("/");
    let actions_fname = [args.dir.as_str(), &args.actions].join("/");
    let flows = from_yaml_file(File::open(&flows_fname)?)?;

    let actions = if std::fs::exists(&actions_fname)? {
        from_yaml_file(File::open(&actions_fname)?)?
    } else {
        info!("File {actions_fname:?} doesn't exist, reading only actions referenced in {flows_fname:?}");
        Actions::default()
    };

    info!("Loading all actions...");
    let loaded_actions = load_actions(&args.dir, actions, &flows)?;
    let zitadel = SimpleZitadelClient::new(args.url, args.token, args.org_id)?;
    info!("Performing sync...");
    sync(false, &zitadel, loaded_actions, flows).await?;
    Ok(())
}

#[allow(clippy::print_stdout, clippy::expect_used)]
pub fn init_tracing(
    level: &tracing_subscriber::filter::LevelFilter,
    additional_env_filters: Option<String>,
) {
    use std::str::FromStr;

    use tracing::Level;
    use tracing_subscriber::{
        filter::LevelFilter, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt,
        EnvFilter,
    };

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
                .with_line_number(level >= &Level::DEBUG)
                .with_span_events(match level {
                    &LevelFilter::TRACE => FmtSpan::CLOSE,
                    _ => FmtSpan::NONE,
                }),
        )
        .with(tracing_error::ErrorLayer::default())
        .try_init()
        .expect("Failed to initialize tracing subscriber");
}
