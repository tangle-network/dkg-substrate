// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use dkg_standalone_runtime::{
	constants::currency::{Balance, DOLLARS},
	AccountId, BalancesConfig, DKGConfig, DKGId, DKGProposalsConfig, GenesisConfig, MaxNominations,
	Perbill, ResourceId, SessionConfig, Signature, StakerStatus, StakingConfig, SudoConfig,
	SystemConfig, WASM_BINARY,
};
use hex_literal::hex;

use sc_service::ChainType;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{sr25519, Pair, Public};
use sp_finality_grandpa::AuthorityId as GrandpaId;
use sp_runtime::traits::{IdentifyAccount, Verify};

// The URL for the telemetry server.
// const STAGING_TELEMETRY_URL: &str = "wss://telemetry.polkadot.io/submit/";

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec<GenesisConfig>;

/// Generate a crypto pair from seed.
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

type AccountPublic = <Signature as Verify>::Signer;

/// Generate an account ID from seed.
pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId
where
	AccountPublic: From<<TPublic::Pair as Pair>::Public>,
{
	AccountPublic::from(get_from_seed::<TPublic>(seed)).into_account()
}

/// Generate an Aura authority key.
pub fn authority_keys_from_seed(
	stash: &str,
	controller: &str,
) -> (AccountId, AccountId, AuraId, GrandpaId, DKGId) {
	(
		get_account_id_from_seed::<sr25519::Public>(stash),
		get_account_id_from_seed::<sr25519::Public>(controller),
		get_from_seed::<AuraId>(stash),
		get_from_seed::<GrandpaId>(stash),
		get_from_seed::<DKGId>(stash),
	)
}

/// Generate the session keys from individual elements.
///
/// The input must be a tuple of individual keys (a single arg for now since we
/// have just one key).
fn dkg_session_keys(
	grandpa: GrandpaId,
	aura: AuraId,
	dkg: DKGId,
) -> dkg_standalone_runtime::opaque::SessionKeys {
	dkg_standalone_runtime::opaque::SessionKeys { grandpa, aura, dkg }
}

pub fn development_config() -> Result<ChainSpec, String> {
	let wasm_binary = WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?;

	Ok(ChainSpec::from_genesis(
		// Name
		"Development",
		// ID
		"dev",
		ChainType::Development,
		move || {
			testnet_genesis(
				wasm_binary,
				// Initial PoA authorities
				vec![
					authority_keys_from_seed("Alice", "Alice//stash"),
					authority_keys_from_seed("Bob", "Bob//stash"),
					authority_keys_from_seed("Charlie", "Charlie//stash"),
				],
				vec![],
				// Sudo account
				get_account_id_from_seed::<sr25519::Public>("Alice"),
				// Pre-funded accounts
				vec![
					get_account_id_from_seed::<sr25519::Public>("Alice"),
					get_account_id_from_seed::<sr25519::Public>("Bob"),
					get_account_id_from_seed::<sr25519::Public>("Charlie"),
					get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
					get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
					get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
					get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
					get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
					get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
				],
				// Initial Chain Ids
				vec![
					hex_literal::hex!("010000001389"), // Hermis (Evm, 5001)
					hex_literal::hex!("01000000138a"), // Athena (Evm, 5002)
				],
				// Initial resource Ids
				vec![
					// Resource ID for Chain Hermis => Athena
					(
						hex_literal::hex!(
							"000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b66701000000138a"
						),
						Default::default(),
					),
					// Resource ID for Chain Athena => Hermis
					(
						hex_literal::hex!(
							"000000000000d30c8839c1145609e564b986f667b273ddcb8496100000001389"
						),
						Default::default(),
					),
				],
				// Initial proposers
				vec![
					get_account_id_from_seed::<sr25519::Public>("Dave"),
					get_account_id_from_seed::<sr25519::Public>("Eve"),
				],
				true,
			)
		},
		// Bootnodes
		vec![],
		// Telemetry
		None,
		// Protocol ID
		None,
		// Fork id
		None,
		// Properties
		None,
		// Extensions
		None,
	))
}

pub fn local_testnet_config() -> Result<ChainSpec, String> {
	let wasm_binary = WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?;

	Ok(ChainSpec::from_genesis(
		// Name
		"Local Testnet",
		// ID
		"local_testnet",
		ChainType::Local,
		move || {
			testnet_genesis(
				wasm_binary,
				// Initial PoA authorities
				vec![
					authority_keys_from_seed("Alice", "Alice//stash"),
					authority_keys_from_seed("Bob", "Bob//stash"),
					authority_keys_from_seed("Charlie", "Charlie//stash"),
				],
				vec![],
				// Sudo account
				get_account_id_from_seed::<sr25519::Public>("Alice"),
				// Pre-funded accounts
				vec![
					get_account_id_from_seed::<sr25519::Public>("Alice"),
					get_account_id_from_seed::<sr25519::Public>("Bob"),
					get_account_id_from_seed::<sr25519::Public>("Charlie"),
					get_account_id_from_seed::<sr25519::Public>("Dave"),
					get_account_id_from_seed::<sr25519::Public>("Eve"),
					get_account_id_from_seed::<sr25519::Public>("Ferdie"),
					get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
					get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
					get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
					get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
					get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
					get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
				],
				// Initial Chain Ids
				vec![
					hex_literal::hex!("010000001389"), // Hermis (Evm, 5001)
					hex_literal::hex!("01000000138a"), // Athena (Evm, 5002)
				],
				// Initial resource Ids
				vec![
					// Resource ID for Chain Hermis => Athena
					(
						hex_literal::hex!(
							"0000000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b6670000138a"
						),
						Default::default(),
					),
					// Resource ID for Chain Athena => Hermis
					(
						hex_literal::hex!(
							"0000000000000000d30c8839c1145609e564b986f667b273ddcb849600001389"
						),
						Default::default(),
					),
				],
				// Initial proposers
				vec![
					get_account_id_from_seed::<sr25519::Public>("Dave"),
					get_account_id_from_seed::<sr25519::Public>("Eve"),
				],
				true,
			)
		},
		// Bootnodes
		vec![],
		// Telemetry
		None,
		// Protocol ID
		None,
		// Fork id
		None,
		// Properties
		None,
		// Extensions
		None,
	))
}

pub fn arana_testnet_config() -> Result<ChainSpec, String> {
	let wasm_binary = WASM_BINARY.ok_or_else(|| "Arana wasm not available".to_string())?;
	let boot_nodes = crate::testnet_fixtures::get_arana_bootnodes();

	Ok(ChainSpec::from_genesis(
		"Arana",
		"arana",
		ChainType::Development,
		move || {
			testnet_genesis(
				wasm_binary,
				// Initial PoA authorities
				crate::testnet_fixtures::get_arana_initial_authorities(),
				vec![],
				// Sudo account
				crate::testnet_fixtures::get_testnet_root_key(),
				// Pre-funded accounts
				vec![
					crate::testnet_fixtures::get_testnet_root_key(),
					hex!["4e85271af1330e5e9384bd3ac5bdc04c0f8ef5a8cc29c1a8ae483d674164745c"].into(),
					hex!["804808fb75d16340dc250871138a1a6f1dfa3cab9cc1fbd6f42960f1c39a950d"].into(),
					hex!["587c2ef00ec0a1b98af4c655763acd76ece690fccbb255f01663660bc274960d"].into(),
					hex!["cc195602a63bbdcf2ef4773c86fdbfefe042cb9aa8e3059d02e59a062d9c3138"].into(),
					hex!["a24f729f085de51eebaeaeca97d6d499761b8f6daeca9b99d754a06ef8bcec3f"].into(),
					hex!["368ea402dbd9c9888ae999d6a799cf36e08673ee53c001dfb4529c149fc2c13b"].into(),
				],
				vec![],
				vec![],
				crate::testnet_fixtures::get_arana_initial_authorities()
					.iter()
					.map(|a| a.0.clone())
					.collect(),
				true,
			)
		},
		// Bootnodes
		boot_nodes,
		// Telemetry
		None,
		// Protocol ID
		None,
		// Fork id
		None,
		// Properties
		None,
		// Extensions
		None,
	))
}

/// Configure initial storage state for FRAME modules.
#[allow(clippy::too_many_arguments)]
fn testnet_genesis(
	wasm_binary: &[u8],
	initial_authorities: Vec<(AccountId, AccountId, AuraId, GrandpaId, DKGId)>,
	initial_nominators: Vec<AccountId>,
	root_key: AccountId,
	endowed_accounts: Vec<AccountId>,
	initial_chain_ids: Vec<[u8; 6]>,
	initial_r_ids: Vec<(ResourceId, Vec<u8>)>,
	initial_proposers: Vec<AccountId>,
	_enable_println: bool,
) -> GenesisConfig {
	const ENDOWMENT: Balance = 10_000_000 * DOLLARS;
	const STASH: Balance = ENDOWMENT / 1000;

	// stakers: all validators and nominators.
	let mut rng = rand::thread_rng();
	let stakers = initial_authorities
		.iter()
		.map(|x| (x.0.clone(), x.1.clone(), STASH, StakerStatus::Validator))
		.chain(initial_nominators.iter().map(|x| {
			use rand::{seq::SliceRandom, Rng};
			let limit = (MaxNominations::get() as usize).min(initial_authorities.len());
			let count = rng.gen::<usize>() % limit;
			let nominations = initial_authorities
				.as_slice()
				.choose_multiple(&mut rng, count)
				.into_iter()
				.map(|choice| choice.0.clone())
				.collect::<Vec<_>>();
			(x.clone(), x.clone(), STASH, StakerStatus::Nominator(nominations))
		}))
		.collect::<Vec<_>>();

	GenesisConfig {
		system: SystemConfig {
			// Add Wasm runtime to storage.
			code: wasm_binary.to_vec(),
		},
		sudo: SudoConfig { key: Some(root_key) },
		transaction_payment: Default::default(),
		balances: BalancesConfig {
			// Configure endowed accounts with initial balance of 1 << 60.
			balances: endowed_accounts.iter().cloned().map(|k| (k, ENDOWMENT)).collect(),
		},
		indices: Default::default(),
		nomination_pools: Default::default(),
		session: SessionConfig {
			keys: initial_authorities
				.iter()
				.map(|x| {
					(
						x.1.clone(),
						x.0.clone(),
						dkg_session_keys(x.3.clone(), x.2.clone(), x.4.clone()),
					)
				})
				.collect::<Vec<_>>(),
		},
		staking: StakingConfig {
			validator_count: initial_authorities.len() as u32,
			minimum_validator_count: initial_authorities.len() as u32 - 1,
			invulnerables: initial_authorities.iter().map(|x| x.0.clone()).collect(),
			slash_reward_fraction: Perbill::from_percent(10),
			stakers,
			..Default::default()
		},
		aura: Default::default(),
		grandpa: Default::default(),
		dkg: DKGConfig {
			authorities: initial_authorities.iter().map(|(.., x)| x.clone()).collect::<_>(),
			keygen_threshold: 2,
			signature_threshold: 1,
			authority_ids: initial_authorities.iter().map(|(x, ..)| x.clone()).collect::<_>(),
		},
		dkg_proposals: DKGProposalsConfig { initial_chain_ids, initial_r_ids, initial_proposers },
	}
}
