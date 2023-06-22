#!/usr/bin/env -S deno run -A --unstable

/// This a script to run and setup the dkg network parachain.

import { ApiPromise, WsProvider, Keyring } from '@polkadot/api';
import { assert, u8aToHex } from '@polkadot/util';
import {
	ProposalHeader,
	ResourceId,
	ChainType,
	AnchorUpdateProposal,
} from '@webb-tools/sdk-core';

async function run() {
	const api = await ApiPromise.create({
		provider: new WsProvider('wss://tangle-standalone-archive.webb.tools'),
	});
	await api.isReady;
	const keyring = new Keyring({ type: 'sr25519' });
	const sudoAccount = keyring.addFromUri('//TangleStandaloneSudo');

	// 000000000000d30c8839c1145609e564b986f667b273ddcb8496010000001389
	const resourceId = ResourceId.newFromContractAddress(
		'0xd30c8839c1145609e564b986f667b273ddcb8496',
		ChainType.EVM,
		5001
	);

	const createHeader = (nonce: number) =>
		new ProposalHeader(resourceId, Buffer.from('0x00000000', 'hex'), nonce);

	// 000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b66701000000138a
	const srcResourceId = ResourceId.newFromContractAddress(
		'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
		ChainType.EVM,
		5002
	);

	// Print resource IDs
	console.log('Resource ID: ', resourceId.toString());
	console.log('Source Resource ID: ', srcResourceId.toString());

	// Create a new anchor proposal every 2 blocks.

	// Fetch account nonce as a starting nonce for the proposals
	const accountNonce = await api.rpc.system.accountNextIndex(
		sudoAccount.address
	);
	let nonce = accountNonce.toNumber();
	while(true) {
		// Create the header
		const proposalHeader = createHeader(nonce);
		assert(
			proposalHeader.toU8a().length === 40,
			`Proposal header should be 40 bytes, instead it is ${
				proposalHeader.toString().length
			} bytes`
		);
		// Create the anchor proposal data structure
		const anchorUpdateProposal: AnchorUpdateProposal = new AnchorUpdateProposal(
			proposalHeader,
			'0x0000000000000000000000000000000000000000000000000000000000000000',
			srcResourceId
		);
		console.log('Proposal Bytes:', u8aToHex(anchorUpdateProposal.toU8a()));
		assert(
			anchorUpdateProposal.toU8a().length === 104,
			`Anchor update proposal should be 104 bytes, instead it is ${
				anchorUpdateProposal.toString().length
			} bytes`
		);
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
		// Sign and send the transaction
		await new Promise(async (resolve, reject) => {
			const unsub = await api.tx.sudo
			  .sudo(call)
			  .signAndSend(sudoAccount, (result) => {
				if (result.isFinalized || result.isInBlock || result.isError) {
				  unsub();
				  console.log(result.txHash.toHex(), "is", result.status.type);
				  resolve(result.isFinalized);
				}
			  });
		  });

		nonce += 1;
	};

	// a perodic task to print How many proposals that have been submitted, unsigned and signed.
	// This is just for debugging purpose.
	// on each new block header, we should print the number of proposals inside the unsigned queue,
	// the number of proposals inside the signed queue, and the ratio between them in percentage.
	await api.rpc.chain.subscribeFinalizedHeads(async (header) => {
		console.log('Current Block Number: ', header.number.toNumber());
		const ourApi = await api.at(header.hash);
		const lastRotation = await ourApi.query.dkg.lastSessionRotationBlock();
		const prevBlockHash = await api.rpc.chain.getBlockHash(
			lastRotation.toPrimitive() as number
		);
		const prevApi = await api.at(prevBlockHash);
		const prevSignedProposalKeys =
			await prevApi.query.dkgProposalHandler.signedProposals.keys({
				Evm: 5001,
			});
		const [unsignedProposalQueueKeys, signedProposalKeys] = await Promise.all([
			ourApi.query.dkgProposalHandler.unsignedProposalQueue.keys({ Evm: 5001 }),
			ourApi.query.dkgProposalHandler.signedProposals.keys({ Evm: 5001 }),
		]);
		const unsignedProposalQueueSize = unsignedProposalQueueKeys.length;
		const signedProposalSize =
			signedProposalKeys.length - prevSignedProposalKeys.length;
		const ratio = (
			(unsignedProposalQueueSize / (signedProposalSize || 1)) *
			100
		).toFixed(2);
		console.log({
			unsignedProposalQueueSize,
			signedProposalSize,
			ratio,
		});
	});
}

run();
