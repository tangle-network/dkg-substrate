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
import 'jest-extended';
import {
	encodeFunctionSignature,
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	registerResourceId,
	waitForEvent,
} from '../src/utils';
import {ethers} from 'ethers';
import {Anchors} from '@webb-tools/protocol-solidity';
import {Keyring} from '@polkadot/api';
import {u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {
	AnchorUpdateProposal,
	encodeUpdateAnchorProposal,
	ChainIdType,
} from '../src/evm/util/utils';
import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	wallet2,
	localChain2, executeAfter
} from './utils/util';

describe('Anchor Update Proposal', () => {
	test('should be able to sign Update Anchor Proposal', async () => {
		// get the anhor on localchain1
		const anchor = signatureBridge.getAnchor(
			localChain.chainId,
			ethers.utils.parseEther('1')
		)! as Anchors.Anchor;
		await anchor.setSigner(wallet1);
		// check the merkle root
		const merkleRoot1 = await anchor.contract.getLastRoot();
		// get the anchor on localchain2
		const anchor2 = signatureBridge.getAnchor(
			localChain2.chainId,
			ethers.utils.parseEther('1')
		)! as Anchors.Anchor;
		await anchor2.setSigner(wallet2);

		// create a deposit on localchain1
		const deposit = await anchor.deposit(localChain2.chainId);
		// now check the new merkel root.
		const newMerkleRoot1 = await anchor.contract.getLastRoot();
		expect(newMerkleRoot1).not.toEqual(merkleRoot1);
		const lastLeafIndex = deposit.index;
		// create anchor update proposal to be sent to the dkg.
		const resourceId = await anchor2.createResourceId();
		const proposalPayload: AnchorUpdateProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					anchor.contract.interface.functions['updateEdge(uint256,bytes32,uint256)'].format()
				),
				nonce: 1,
				chainId: localChain2.chainId,
				chainIdType: ChainIdType.EVM,
			},
			chainIdType: ChainIdType.EVM,
			srcChainId: localChain.chainId,
			merkleRoot: newMerkleRoot1,
			lastLeafIndex,
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeUpdateAnchorProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({type: 'sr25519'});
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
			Evm: localChain2.chainId,
		});
		const proposalCall = polkadotApi.tx.dKGProposals.acknowledgeProposal(
			proposalPayload.header.nonce,
			chainIdType,
			resourceId,
			prop
		);
		const tx = new Promise<void>(async (resolve, reject) => {
			const unsub = await proposalCall.signAndSend(alice, ({events, status}) => {
				if (status.isFinalized) {
					unsub();
					const success = events.find(({event}) =>
						polkadotApi.events.system.ExtrinsicSuccess.is(event)
					);
					if (success) {
						resolve();
					} else {
						reject(new Error('Proposal failed'));
					}
				}
			});
		});
		await expect(tx).toResolve();
		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
		// now we need to query the proposal and its signature.
		const key = {
			AnchorUpdateProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
		expect(value.isSome).toBeTrue();
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'AnchorUpdate';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).toEqual(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = signatureBridge.getBridgeSide(localChain2.chainId)!;
		// but first, we need to log few things to help us to debug.
		let wallet2_ = wallet2.connect(localChain2.provider());
		const contract = bridgeSide.contract.connect(wallet2_);
		const currentGovernor = await contract.governor();
		const currentDkgPublicKey = await fetchDkgPublicKey(polkadotApi);
		const currentDkgAddress = ethAddressFromUncompressedPublicKey(currentDkgPublicKey!);
		expect(currentGovernor).toEqualCaseInsensitive(currentDkgAddress);
		// now we log the proposal data, signature, and if it is signed by the current governor or not.
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		// check that we have the resouceId mapping.
		const val = await contract._resourceIDToHandlerAddress(resourceId);
		const anchorHandlerAddress = await anchor2.getHandler();
		expect(val).toEqual(anchorHandlerAddress);
		const tx2 = await contract.executeProposalWithSignature(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		await expect(tx2.wait()).toResolve();
		// now we shall check the new merkle root on the other chain.
		const newMerkleRoots = await anchor2.contract.getLatestNeighborRoots();
		// the merkle root should be included now.
		expect(newMerkleRoots.includes(newMerkleRoot1)).toBeTrue();
	});

	afterAll(async () => {
		await executeAfter();
	});
});
