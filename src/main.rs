#![allow(missing_docs, clippy::missing_docs_in_private_items)]
use clap::Parser;
use famedly_rust_utils::BaseUrl;
use zitadel_actions_sync::{load_actions, sync, zitadel::SimpleZitadelClient};

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
	#[arg(short, long, default_value = "localhost:9310")]
	url: BaseUrl,

	/// Zitadel access token
	#[arg(short, long)]
	token: String,

	/// Organization for which perform the sync
	#[arg(short, long)]
	org_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();

	let flows = serde_yaml::from_reader(std::fs::File::open(args.flows)?)?;
	let actions = serde_yaml::from_reader(std::fs::File::open(args.actions)?)?;

	let loaded_actions = load_actions(&args.dir, actions, &flows)?;
	let zitadel = SimpleZitadelClient::new(args.url, args.token, args.org_id)?;
	sync(false, &zitadel, loaded_actions, flows).await?;
	Ok(())
}
