//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use dkg_gadget::DKG_PROTOCOL_NAME;
use dkg_standalone_runtime::{self, opaque::Block, RuntimeApi};
use sc_client_api::{BlockBackend, ExecutorProvider};
use sc_consensus_aura::{ImportQueueParams, SlotProportion, StartAuraParams};
use sc_consensus_manual_seal::consensus::timestamp::SlotTimestampProvider;
use sc_executor::NativeElseWasmExecutor;
use sc_finality_grandpa::SharedVoterState;
use sc_keystore::LocalKeystore;
use sc_service::{error::Error as ServiceError, BasePath, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sp_consensus::SlotData;
use sp_consensus_aura::sr25519::AuthorityPair as AuraPair;
use std::{sync::Arc, time::Duration};

/// Native executor instance.
pub struct RuntimeExecutor;

impl sc_executor::NativeExecutionDispatch for RuntimeExecutor {
	type ExtendHostFunctions = frame_benchmarking::benchmarking::HostFunctions;

	fn dispatch(method: &str, data: &[u8]) -> Option<Vec<u8>> {
		dkg_standalone_runtime::api::dispatch(method, data)
	}

	fn native_version() -> sc_executor::NativeVersion {
		dkg_standalone_runtime::native_version()
	}
}
pub type Executor = NativeElseWasmExecutor<RuntimeExecutor>;

type FullClient = sc_service::TFullClient<Block, RuntimeApi, Executor>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;

#[cfg(not(feature = "manual-seal"))]
type Extra = (
	sc_finality_grandpa::GrandpaBlockImport<FullBackend, Block, FullClient, FullSelectChain>,
	sc_finality_grandpa::LinkHalf<Block, FullClient, FullSelectChain>,
	Option<Telemetry>,
);

#[cfg(feature = "manual-seal")]
type Extra = Option<Telemetry>;

pub fn new_partial(
	config: &Configuration,
) -> Result<
	sc_service::PartialComponents<
		FullClient,
		FullBackend,
		FullSelectChain,
		sc_consensus::DefaultImportQueue<Block, FullClient>,
		sc_transaction_pool::FullPool<Block, FullClient>,
		Extra,
	>,
	ServiceError,
> {
	if config.keystore_remote.is_some() {
		return Err(ServiceError::Other(format!("Remote Keystores are not supported.")))
	}

	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let executor = sc_executor::NativeElseWasmExecutor::<RuntimeExecutor>::new(
		config.wasm_method,
		config.default_heap_pages,
		config.max_runtime_instances,
		config.runtime_cache_size,
	);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, Executor>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;
	let client = Arc::new(client);

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let select_chain = sc_consensus::LongestChain::new(backend.clone());

	let transaction_pool = sc_transaction_pool::BasicPool::new_full(
		config.transaction_pool.clone(),
		config.role.is_authority().into(),
		config.prometheus_registry(),
		task_manager.spawn_essential_handle(),
		client.clone(),
	);

	#[cfg(not(feature = "manual-seal"))]
	let (grandpa_block_import, grandpa_link) = sc_finality_grandpa::block_import(
		client.clone(),
		&(client.clone() as Arc<_>),
		select_chain.clone(),
		telemetry.as_ref().map(|x| x.handle()),
	)?;

	#[cfg(feature = "manual-seal")]
	let import_queue = sc_consensus_manual_seal::import_queue(
		Box::new(client.clone()),
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
	);

	#[cfg(not(feature = "manual-seal"))]
	let slot_duration = sc_consensus_aura::slot_duration(&*client)?.slot_duration();

	#[cfg(not(feature = "manual-seal"))]
	let import_queue =
		sc_consensus_aura::import_queue::<AuraPair, _, _, _, _, _, _>(ImportQueueParams {
			block_import: grandpa_block_import.clone(),
			justification_import: Some(Box::new(grandpa_block_import.clone())),
			client: client.clone(),
			create_inherent_data_providers: move |_, ()| async move {
				let timestamp = sp_timestamp::InherentDataProvider::from_system_time();

				let slot =
					sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_duration(
						*timestamp,
						slot_duration,
					);

				Ok((timestamp, slot))
			},
			spawner: &task_manager.spawn_essential_handle(),
			can_author_with: sp_consensus::CanAuthorWithNativeVersion::new(
				client.executor().clone(),
			),
			registry: config.prometheus_registry(),
			check_for_equivocation: Default::default(),
			telemetry: telemetry.as_ref().map(|x| x.handle()),
		})?;

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		#[cfg(not(feature = "manual-seal"))]
		other: (grandpa_block_import, grandpa_link, telemetry),
		#[cfg(feature = "manual-seal")]
		other: telemetry,
	})
}

fn remote_keystore(_url: &String) -> Result<Arc<LocalKeystore>, &'static str> {
	// FIXME: here would the concrete keystore be built,
	//        must return a concrete type (NOT `LocalKeystore`) that
	//        implements `CryptoStore` and `SyncCryptoStore`
	Err("Remote Keystore not supported.")
}

/// Builds a new service for a full client.
pub fn new_full(mut config: Configuration) -> Result<TaskManager, ServiceError> {
	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		mut keystore_container,
		select_chain,
		transaction_pool,
		other,
	} = new_partial(&config)?;

	#[cfg(not(feature = "manual-seal"))]
	let (block_import, grandpa_link, mut telemetry) = other;

	#[cfg(feature = "manual-seal")]
	let mut telemetry = other;

	if let Some(url) = &config.keystore_remote {
		match remote_keystore(url) {
			Ok(k) => keystore_container.set_remote_keystore(k),
			Err(e) =>
				return Err(ServiceError::Other(format!(
					"Error hooking up remote keystore for {}: {}",
					url, e
				))),
		};
	}

	let grandpa_protocol_name = sc_finality_grandpa::protocol_standard_name(
		&client.block_hash(0).ok().flatten().expect("Genesis block exists; qed"),
		&config.chain_spec,
	);

	config
		.network
		.extra_sets
		.push(sc_finality_grandpa::grandpa_peers_set_config(grandpa_protocol_name.clone()));
	config.network.extra_sets.push(dkg_gadget::dkg_peers_set_config());

	let (network, system_rpc_tx, network_starter) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync: None,
		})?;

	if config.offchain_worker.enabled {
		sc_service::build_offchain_workers(
			&config,
			task_manager.spawn_handle(),
			client.clone(),
			network.clone(),
		);
	}

	let role = config.role.clone();
	let force_authoring = config.force_authoring;
	let backoff_authoring_blocks: Option<()> = None;
	let name = config.network.node_name.clone();
	let enable_grandpa = !config.disable_grandpa;
	let prometheus_registry = config.prometheus_registry().cloned();

	let base_path = if config.base_path.is_some() {
		match config.base_path.as_ref() {
			Some(BasePath::Permanenent(path_buf)) => Some(path_buf.clone()),
			_ => None,
		}
	} else {
		None
	};

	if role.is_authority() {
		dkg_primitives::utils::insert_controller_account_keys_into_keystore(
			&config,
			Some(keystore_container.sync_keystore()),
		);
	}

	// if the node isn't actively participating in consensus then it doesn't
	// need a keystore, regardless of which protocol we use below.
	let keystore =
		if role.is_authority() { Some(keystore_container.sync_keystore()) } else { None };

	let rpc_client = client.clone();
	let command_sink = if role.is_authority() {
		let dkg_params = dkg_gadget::DKGParams {
			client: client.clone(),
			backend: backend.clone(),
			key_store: keystore.clone(),
			network: network.clone(),
			min_block_delta: 4,
			prometheus_registry: prometheus_registry.clone(),
			base_path,
			local_keystore: keystore_container.local_keystore(),
			_block: std::marker::PhantomData::<Block>,
		};

		// Start the DKG gadget.
		task_manager.spawn_essential_handle().spawn_blocking(
			"dkg-gadget",
			None,
			dkg_gadget::start_dkg_gadget::<_, _, _, _>(dkg_params),
		);

		let proposer_factory = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|x| x.handle()),
		);

		if cfg!(feature = "manual-seal") {
			use futures::prelude::*;
			// Channel for the rpc handler to communicate with the authorship task.
			let (command_sink, commands_stream) = futures::channel::mpsc::channel(1024);

			let pool = transaction_pool.pool().clone();
			let import_stream = pool.validated_pool().import_notification_stream().map(|_| {
				sc_consensus_manual_seal::rpc::EngineCommand::SealNewBlock {
					create_empty: false,
					finalize: true,
					parent_hash: None,
					sender: None,
				}
			});
			let authorship_future = sc_consensus_manual_seal::run_manual_seal(
				sc_consensus_manual_seal::ManualSealParams {
					block_import: client.clone(),
					env: proposer_factory,
					client,
					pool: transaction_pool.clone(),
					commands_stream: futures::stream_select!(commands_stream, import_stream),
					select_chain,
					consensus_data_provider: None,
					create_inherent_data_providers: Box::new(move |_, _| async move {
						let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
						Ok(timestamp)
					}),
				},
			);
			// we spawn the future on a background thread managed by service.
			task_manager.spawn_essential_handle().spawn_blocking(
				"manual-seal",
				Some("block-authoring"),
				authorship_future,
			);
			Some(command_sink)
		} else {
			#[cfg(not(feature = "manual-seal"))]
			{
				let can_author_with =
					sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone());

				let slot_duration = sc_consensus_aura::slot_duration(&*client)?;
				let raw_slot_duration = slot_duration.slot_duration();

				let aura =
					sc_consensus_aura::start_aura::<AuraPair, _, _, _, _, _, _, _, _, _, _, _>(
						StartAuraParams {
							slot_duration,
							client,
							select_chain,
							block_import,
							proposer_factory,
							create_inherent_data_providers: move |_, ()| async move {
								let timestamp =
									sp_timestamp::InherentDataProvider::from_system_time();

								let slot =
						sp_consensus_aura::inherents::InherentDataProvider::from_timestamp_and_duration(
							*timestamp,
							raw_slot_duration,
						);

								Ok((timestamp, slot))
							},
							force_authoring,
							backoff_authoring_blocks,
							keystore: keystore_container.sync_keystore(),
							can_author_with,
							sync_oracle: network.clone(),
							justification_sync_link: network.clone(),
							block_proposal_slot_portion: SlotProportion::new(2f32 / 3f32),
							max_block_proposal_slot_portion: None,
							telemetry: telemetry.as_ref().map(|x| x.handle()),
						},
					)?;

				// the AURA authoring task is considered essential, i.e. if it
				// fails we take down the service with it.
				task_manager.spawn_essential_handle().spawn_blocking(
					"aura",
					Some("block-authoring"),
					aura,
				);
			}
			None
		}
	} else {
		None
	};

	let rpc_extensions_builder = {
		let client = rpc_client.clone();
		let pool = transaction_pool.clone();

		Box::new(move |deny_unsafe, _| {
			let deps = crate::rpc::FullDeps {
				client: client.clone(),
				pool: pool.clone(),
				command_sink: command_sink.clone(),
				deny_unsafe,
			};

			Ok(crate::rpc::create_full(deps))
		})
	};

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network: network.clone(),
		client: rpc_client,
		keystore: keystore_container.sync_keystore(),
		task_manager: &mut task_manager,
		transaction_pool,
		rpc_extensions_builder,
		backend,
		system_rpc_tx,
		config,
		telemetry: telemetry.as_mut(),
	})?;

	#[cfg(not(feature = "manual-seal"))]
	let grandpa_config = sc_finality_grandpa::Config {
		// FIXME #1578 make this available through chainspec
		gossip_duration: Duration::from_millis(333),
		justification_period: 512,
		name: Some(name),
		observer_enabled: false,
		keystore,
		local_role: role,
		telemetry: telemetry.as_ref().map(|x| x.handle()),
		protocol_name: grandpa_protocol_name,
	};

	#[cfg(not(feature = "manual-seal"))]
	if enable_grandpa {
		// start the full GRANDPA voter
		// NOTE: non-authorities could run the GRANDPA observer protocol, but at
		// this point the full voter should provide better guarantees of block
		// and vote data availability than the observer. The observer has not
		// been tested extensively yet and having most nodes in a network run it
		// could lead to finality stalls.
		let grandpa_config = sc_finality_grandpa::GrandpaParams {
			config: grandpa_config,
			link: grandpa_link,
			network,
			voting_rule: sc_finality_grandpa::VotingRulesBuilder::default().build(),
			prometheus_registry,
			shared_voter_state: SharedVoterState::empty(),
			telemetry: telemetry.as_ref().map(|x| x.handle()),
		};

		// the GRANDPA voter task is considered infallible, i.e.
		// if it fails we take down the service with it.
		task_manager.spawn_essential_handle().spawn_blocking(
			"grandpa-voter",
			None,
			sc_finality_grandpa::run_grandpa_voter(grandpa_config)?,
		);
	}

	network_starter.start_network();
	Ok(task_manager)
}
