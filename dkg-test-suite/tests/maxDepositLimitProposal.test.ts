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
	sudoTx,
	waitForEvent,
} from './utils/setup';
import { Keyring } from '@polkadot/api';
import { hexToNumber, u8aToHex, hexToU8a } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import { localChain, polkadotApi, signatureVBridge } from './utils/util';
import { expect } from 'chai';
import { ChainType, MaxDepositLimitProposal, ProposalHeader, ResourceId } from '@webb-tools/sdk-core';
import { registerResourceId } from '@webb-tools/test-utils';

it('should be able to update max deposit limit', async () => {
	const vAnchor = signatureVBridge.getVAnchor(localChain.typedChainId)!;
	const resourceId = ResourceId.newFromContractAddress(vAnchor.getAddress(), ChainType.EVM, localChain.evmId);
	// Create Mintable Token to add to GovernedTokenWrapper
	//Create an ERC20 Token
	const functionSignature = hexToU8a(vAnchor.contract.interface.functions['configureMaximumDepositLimit(uint256,uint32)'].format());
	const nonce = Number(await vAnchor.contract.getProposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(resourceId, functionSignature, nonce);
	const maxLimitProposal = new MaxDepositLimitProposal(proposalHeader, '0x50000000');

	// register proposal resourceId.
	await registerResourceId(polkadotApi, maxLimitProposal.header.resourceId);
	const proposalBytes = maxLimitProposal.toU8a();
	// get alice account to send the transaction to the dkg node.
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const prop = u8aToHex(proposalBytes);
	const chainIdType = polkadotApi.createType(
		'WebbProposalsHeaderTypedChainId',
		{
			Evm: localChain.evmId,
		}
	);
	const kind = polkadotApi.createType(
		'WebbProposalsProposalProposalKind',
		'MaxDepositLimitUpdate'
	);
	const maxDepositLimitProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: kind,
				data: prop,
			},
		}
	);
	const proposalCall =
		polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(
			maxDepositLimitProposal
		);

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

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
		key: 'MaxDepositLimitUpdateProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		MaxDepositLimitUpdateProposal: maxLimitProposal.header.nonce,
	};
	const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(
		chainIdType,
		key
	);
	const value = new Option(
		polkadotApi.registry,
		'WebbProposalsProposal',
		proposal
	);
	expect(value.isSome).to.eq(true);
	const dkgProposal = value.unwrap().toJSON() as {
		signed: {
			kind: 'MaxDepositLimitUpdate';
			data: HexString;
			signature: HexString;
		};
	};
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
	const contract = bridgeSide.contract;

	const governor = await contract.governor();
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.signed.data,
		dkgProposal.signed.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const tx2 = await contract.executeProposalWithSignature(
		dkgProposal.signed.data,
		dkgProposal.signed.signature
	);
	await tx2.wait();
	// Want to check that fee was updated
	const maxDepositLimit = await vAnchor.contract.maximumDepositAmount();
	expect(hexToNumber('0x50000000').toString()).to.eq(
		maxDepositLimit.toString()
	);
});
