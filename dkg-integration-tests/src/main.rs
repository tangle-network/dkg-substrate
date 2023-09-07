use ac_primitives::{AssetRuntimeConfig, H256};
use std::{collections::HashSet, error::Error, time::Duration};
use substrate_api_client::{rpc::JsonrpseeClient, Api, SubscribeEvents};
use tokio::{net::TcpStream, process::Child};
use tokio_websockets::{ClientBuilder, MaybeTlsStream, WebsocketStream};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut shutdown = Shutdown { members: vec![] };
	let res = main_inner(&mut shutdown).await;

	for member in shutdown.members.iter_mut() {
		if let Some(pid) = member.id() {
			dkg_logging::info!(target: "dkg", "Killing member {pid}");
			let _exit = tokio::process::Command::new("kill")
				.arg("-9")
				.arg(pid.to_string())
				.stderr(std::process::Stdio::inherit())
				.stdout(std::process::Stdio::inherit())
				.spawn()
				.expect("Should spawn")
				.wait()
				.await
				.unwrap();
		}

		member.kill().await.expect("Should kill member");
	}

	res
}

async fn main_inner(shutdown: &mut Shutdown) -> Result<(), Box<dyn Error>> {
	dkg_logging::setup_log();
	dkg_logging::info!(target: "dkg", "Will test getting to session 3");

	let alice = generate_process("alice", false)?;
	dkg_logging::info!(target: "dkg", "Spawned Bootnode, now awaiting for initialization");
	// With alice spawned, we now must wait for her to be ready to accept connections
	wait_for_bootnode_to_init().await?;

	// with the bootnode ready, load the rest of the nodes
	let bob = generate_process("bob", false)?;
	let charlie = generate_process("charlie", false)?;
	let dave = generate_process("dave", false)?;
	let eve = generate_process("eve", false)?;

	shutdown.members = vec![alice, bob, charlie, dave, eve];

	// Setup API
	let client = JsonrpseeClient::with_default_url().unwrap();
	let mut api = Api::<ac_primitives::AssetRuntimeConfig, _>::new(client).unwrap();

	// Wait for the bootnode to get to session 5
	wait_for_session_3(&mut api).await;
	Ok(())
}

async fn wait_for_bootnode_to_init() -> Result<(), Box<dyn Error>> {
	loop {
		match tokio::time::timeout(
			Duration::from_millis(10000),
			generate_websocket_connection_to_bootnode(),
		)
		.await
		{
			Ok(Ok(_ws)) => {
				dkg_logging::info!(target: "dkg", "Obtained websocket connection to bootnode");
				return Ok(())
			},

			Ok(Err(_err)) => {},

			Err(_err) => {
				dkg_logging::info!(target: "dkg", "Still waiting for bootnode to init ...");
			},
		}
	}
}

async fn generate_websocket_connection_to_bootnode(
) -> Result<WebsocketStream<MaybeTlsStream<TcpStream>>, Box<dyn Error>> {
	Ok(ClientBuilder::new()
		.uri("ws://127.0.0.1:9944")? // connect to the bootnode's WS port to listen to events
		.connect()
		.await?)
}

async fn wait_for_session_3(api: &mut Api<AssetRuntimeConfig, JsonrpseeClient>) {
	let mut has_seen = HashSet::new();
	let mut listener = DKGEventListener::new();
	loop {
		let mut subscription = api.subscribe_events().unwrap();
		let records = tokio::task::spawn_blocking(move || {
			let events = subscription.next_events::<dkg_standalone_runtime::RuntimeEvent, H256>();
			if let Some(events) = events {
				events.unwrap_or_default()
			} else {
				vec![]
			}
		})
		.await
		.expect("JoinError");

		for record in records {
			let record_str = format!("{record:?}");
			if has_seen.insert(record_str.clone()) {
				dkg_logging::info!(target: "dkg", "decoded: {:?}", record.event);
				let session = listener.on_event_received(&record_str);
				dkg_logging::info!(target: "dkg", "State: {listener:?}");
				if session == 3 {
					return
				}
			}
		}
	}
}

fn generate_process(node: &str, inherit: bool) -> Result<Child, Box<dyn Error>> {
	let mut command = tokio::process::Command::new("cargo");

	if inherit {
		command.stdout(std::process::Stdio::inherit());
		command.stderr(std::process::Stdio::inherit());
	} else {
		command.stdout(std::process::Stdio::piped());
		command.stderr(std::process::Stdio::piped());
	}

	Ok(command
		.arg("make")
		.arg(node)
		.env("RUST_LOG", "dkg=info")
		.kill_on_drop(true)
		.spawn()?)
}

struct Shutdown {
	members: Vec<Child>,
}

const EVENT_DKG_PUBLIC_KEY_SUBMITTED: &str = "Event::PublicKeySubmitted";
const EVENT_NEXT_DKG_PUBLIC_KEY_SUBMITTED: &str = "Event::NextPublicKeySubmitted";
const EVENT_DKG_PROPOSAL_ADDED: &str = "Event::ProposalAdded";
const EVENT_DKG_PROPOSAL_BATCH_SIGNED: &str = "Event::ProposalBatchSigned";
const EVENT_NEXT_DKG_PUBLIC_KEY_SIGNATURE_SUBMITTED: &str =
	"Event::NextPublicKeySignatureSubmitted";
const EVENT_NEW_SESSION: &str = "Event::NewSession";
const EVENT_PUBLIC_KEY_CHANGED: &str = "Event::PublicKeyChanged";
const EVENT_PUBLIC_KEY_SIGNATURE_CHANGED: &str = "Event::PublicKeySignatureChanged";

#[derive(Debug)]
struct DKGEventListener {
	current_session: u64,
	current_session_events_required: HashSet<&'static str>,
}

impl DKGEventListener {
	fn new() -> Self {
		Self {
			current_session: 0,
			current_session_events_required: [
				EVENT_DKG_PUBLIC_KEY_SUBMITTED,
				EVENT_NEXT_DKG_PUBLIC_KEY_SUBMITTED,
				EVENT_DKG_PROPOSAL_ADDED,
				EVENT_DKG_PROPOSAL_BATCH_SIGNED,
				EVENT_NEXT_DKG_PUBLIC_KEY_SIGNATURE_SUBMITTED,
				EVENT_NEW_SESSION,
				EVENT_PUBLIC_KEY_CHANGED,
				EVENT_PUBLIC_KEY_SIGNATURE_CHANGED,
			]
			.into_iter()
			.collect(),
		}
	}
	// Returns the current session
	fn on_event_received(&mut self, event: &str) -> u64 {
		let maybe_matched_event = self
			.current_session_events_required
			.iter()
			.find(|exact| event.contains(**exact));
		if let Some(matched_event) = maybe_matched_event {
			self.current_session_events_required.remove(*matched_event);
			if self.current_session_events_required.is_empty() {
				self.current_session += 1;
				self.current_session_events_required = [
					// EVENT_DKG_PUBLIC_KEY_SUBMITTED, omit this since this is only needed in the
					// zeroth session (genesis)
					EVENT_NEXT_DKG_PUBLIC_KEY_SUBMITTED,
					EVENT_DKG_PROPOSAL_ADDED,
					EVENT_DKG_PROPOSAL_BATCH_SIGNED,
					EVENT_NEXT_DKG_PUBLIC_KEY_SIGNATURE_SUBMITTED,
					EVENT_NEW_SESSION,
					EVENT_PUBLIC_KEY_CHANGED,
					EVENT_PUBLIC_KEY_SIGNATURE_CHANGED,
				]
				.into_iter()
				.collect();
			}
		}

		self.current_session
	}
}
