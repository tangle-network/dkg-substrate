#!/usr/bin/env -S deno run -A --unstable
/**
The script connects to a Polkadot node, creates and submits batches of proposals, 
and periodically prints debugging information about the proposal status.
*/
import { ApiPromise, WsProvider, Keyring } from '@polkadot/api';
import { assert, u8aToHex } from '@polkadot/util';
import {
	ProposalHeader,
	ResourceId,
	ChainType,
	AnchorUpdateProposal,
} from '@webb-tools/sdk-core';

// Number of proposals to send in one batch
const PROPOSALS_TO_SEND_IN_ONE_BATCH = 10;
// Node URL for the API connection
const NODE_URL = "wss://tangle-standalone-archive.webb.tools";
// Private key of the signer account
const SIGNER_PVT_KEY = "//Alice";
// Delay between proposal submission
const DELAY_IN_MS = 5000;

async function run() {
	// Create an instance of the Polkadot API
	const api = await ApiPromise.create({
		provider: new WsProvider(NODE_URL),
	});
	await api.isReady;

	// Create a keyring instance to manage accounts
	const keyring = new Keyring({ type: 'sr25519' });

	// Get the signer account from the private key
	const sudoAccount = keyring.addFromUri(SIGNER_PVT_KEY);

	// Create a resource ID for the main contract
	const resourceId = ResourceId.newFromContractAddress(
		'0xd30c8839c1145609e564b986f667b273ddcb8496',
		ChainType.EVM,
		1337
	);

	// Create a header for the proposal with a given nonce
	const createHeader = (nonce: number) =>
		new ProposalHeader(resourceId, Buffer.from('0x00000000', 'hex'), nonce);

	// Create a resource ID for the source contract
	const srcResourceId = ResourceId.newFromContractAddress(
		'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
		ChainType.EVM,
		1338
	);

	// Print resource IDs
	console.log('Resource ID: ', resourceId.toString());
	console.log('Source Resource ID: ', srcResourceId.toString());

	// Fetch account nonce as a starting nonce for the proposals
	const accountNonce = await api.rpc.system.accountNextIndex(
		sudoAccount.address
	);
	let nonce = accountNonce.toNumber();

	// Send a batch of proposals every block
	while (true) {
		let calls = [];

		for (let i = 0; i < PROPOSALS_TO_SEND_IN_ONE_BATCH; i++) {
			// Create the header for the proposal
			const proposalHeader = createHeader(nonce);

			// Ensure the proposal header has the correct length
			assert(
				proposalHeader.toU8a().length === 40,
				`Proposal header should be 40 bytes, instead it is ${proposalHeader.toString().length} bytes`
			);

			// Create the anchor update proposal data structure
			const anchorUpdateProposal: AnchorUpdateProposal = new AnchorUpdateProposal(
				proposalHeader,
				'0x0000000000000000000000000000000000000000000000000000000000000000',
				srcResourceId
			);

			// Print the proposal bytes
			console.log('Proposal Bytes:', u8aToHex(anchorUpdateProposal.toU8a()));

			// Ensure the anchor update proposal has the correct length
			assert(
				anchorUpdateProposal.toU8a().length === 104,
				`Anchor update proposal should be 104 bytes, instead it is ${anchorUpdateProposal.toString().length} bytes`
			);

			// Create the proposal kind and data
			const kind = api.createType(
				'WebbProposalsProposalProposalKind',
				'AnchorUpdate'
			);
			const prop = api.createType('WebbProposalsProposal', {
				Unsigned: {
					kind,
					data: u8aToHex(anchorUpdateProposal.toU8a()),
				},
			});

			// Submit the unsigned proposal to the chain
			const call = api.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
				prop.toU8a()
			);

			calls.push(call);
		}

		// Create a batch call with all the proposals
		let batch_call = api.tx.utility.batch(calls);

		// Sign and send the transaction as one batch
		await new Promise(async (resolve, reject) => {
			const unsub = await api.tx.sudo
				.sudo(batch_call)
				.signAndSend(sudoAccount, (result) => {
					if (result.isFinalized || result.isInBlock || result.isError) {
						unsub();
						console.log(result.txHash.toHex(), "is", result.status.type);
						resolve(result.isFinalized);
					}
				});
		});

		nonce += 1;

		console.log("Waiting for ", DELAY_IN_MS);
		await new Promise(resolve => setTimeout(resolve, DELAY_IN_MS));
	}
}

run();