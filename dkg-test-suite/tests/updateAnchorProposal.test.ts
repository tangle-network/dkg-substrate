/*
 * Copyright 2022 Webb Technologies Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
import {
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	sudoTx,
	waitForEvent,
} from './utils/setup';
import { u8aToHex, hexToU8a } from '@polkadot/util';
import {
	ChainType,
	ResourceId,
	ProposalHeader,
	AnchorUpdateProposal,
	CircomUtxo
} from '@webb-tools/sdk-core';
import { 
	registerResourceId
} from '@webb-tools/test-utils';
import {
	localChain,
	polkadotApi,
	signatureVBridge,
	wallet1,
	wallet2,
	localChain2,
} from './utils/util';
import { expect } from 'chai';
import { Keyring } from '@polkadot/api';

it('should be able to sign anchor update proposal', async () => {
	// get the anhor on localchain1
	const anchor = signatureVBridge.getVAnchor(
		localChain.typedChainId
	)!;
	await anchor.setSigner(wallet1);
	// check the merkle root
	const merkleRoot1 = await anchor.contract.getLastRoot();
	// get the anchor on localchain2
	const anchor2 = signatureVBridge.getVAnchor(
		localChain2.typedChainId
	)!;
	await anchor2.setSigner(wallet2);

	// create a deposit on localchain1
	const outputUtxo = await CircomUtxo.generateUtxo({
		amount: '10000000',
		backend: 'Circom',
		chainId: localChain.typedChainId.toString(),
		curve: 'Bn254',
	});
	await anchor.transact(
		[],
		[outputUtxo],
		{},
		0,
		wallet1.address,
		wallet1.address,
	);

	// now check the new merkel root.
	const newMerkleRoot1 = await anchor.contract.getLastRoot();
	expect(newMerkleRoot1).not.to.eq(merkleRoot1);
	// create anchor update proposal to be sent to the dkg.
	const resourceId = ResourceId.newFromContractAddress(anchor2.contract.address, ChainType.EVM, localChain2.evmId);
	const functionSignature = hexToU8a(anchor.contract.interface.getSighash('updateEdge(bytes32,uint32,bytes32)'));
	const nonce = Number(await anchor.contract.getProposalNonce()) + 1
	const proposalHeader = new ProposalHeader(resourceId, functionSignature, nonce);

	const srcResourceId = ResourceId.newFromContractAddress(anchor.contract.address, ChainType.EVM, localChain.evmId);

	const anchorProposal: AnchorUpdateProposal = new AnchorUpdateProposal(
		proposalHeader,
		newMerkleRoot1,
		srcResourceId
	);
	// register proposal resourceId.
	await registerResourceId(polkadotApi, anchorProposal.header.resourceId);
	// get alice account to send the transaction to the dkg node.
	const prop = u8aToHex(anchorProposal.toU8a());
	const proposalCall = polkadotApi.tx.dkgProposals.acknowledgeProposal(
		anchorProposal.header.nonce,
		{
			Evm: localChain.evmId,
		},
		resourceId.toU8a(),
		prop
	);
	
	// The acknowledgeProposal call should come from someone in the proposer set
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const tx = new Promise<void>(async (resolve, reject) => {
		const unsub = await proposalCall.signAndSend(
			alice,
			({ events, status }) => {
				if (status.isFinalized) {
					unsub();
					const success = events.find(({ event }) =>
						polkadotApi.events.system.ExtrinsicSuccess.is(event)
					);
					if (success) {
						resolve();
					} else {
						reject(new Error('Proposal failed'));
					}
				}
			}
		);
	});
	await tx;

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
		key: 'anchorUpdateProposal',
	});

	console.log('after wait for Event');

	// now we need to query the proposal and its signature.
	const key = {
		AnchorUpdateProposal: anchorProposal.header.nonce,
	};
	const proposal = await polkadotApi.query.dkgProposalHandler.signedProposals(
		{
			Evm: localChain2.evmId,
		},
		key
	);

	const dkgProposal = proposal.unwrap().asSigned;
	// sanity check.
	expect(u8aToHex(dkgProposal.data)).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = signatureVBridge.getVBridgeSide(localChain2.typedChainId)!;
	// but first, we need to log few things to help us to debug.
	let wallet2_ = wallet2.connect(localChain2.provider());
	const contract = bridgeSide.contract.connect(wallet2_);
	const currentGovernor = await contract.governor();
	const currentDkgPublicKey = await fetchDkgPublicKey(polkadotApi);
	const currentDkgAddress = ethAddressFromUncompressedPublicKey(
		currentDkgPublicKey!
	);
	expect(currentGovernor).to.eq(currentDkgAddress);
	// now we log the proposal data, signature, and if it is signed by the current governor or not.
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.data,
		dkgProposal.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const val = await contract._resourceIDToHandlerAddress(resourceId.toString());
	const anchorHandlerAddress = await anchor2.getHandler();
	expect(val).to.eq(anchorHandlerAddress);
	const tx2 = await contract.executeProposalWithSignature(
		dkgProposal.data,
		dkgProposal.signature
	);
	await tx2.wait();
	// now we shall check the new merkle root on the other chain.
	const newMerkleRoots = await anchor2.contract.getLatestNeighborRoots();
	// the merkle root should be included now.
	expect(newMerkleRoots.includes(newMerkleRoot1)).to.eq(true);
});
