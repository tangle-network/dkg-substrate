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
import '@polkadot/api-augment';
import '@webb-tools/dkg-substrate-types';
import { sudoTx, waitForEvent } from './utils/setup';
import { hexToNumber, u8aToHex, hexToU8a } from '@polkadot/util';
import { localChain, polkadotApi, signatureVBridge } from './utils/util';
import { expect } from 'chai';
import {
	ChainType,
	MaxDepositLimitProposal,
	ProposalHeader,
	ResourceId,
} from '@webb-tools/sdk-core';
import { registerResourceId } from '@webb-tools/test-utils';
import type { WebbProposalsProposal } from '@polkadot/types/lookup';

it('should be able to update max deposit limit', async () => {
	const vAnchor = signatureVBridge.getVAnchor(localChain.typedChainId)!;
	const resourceId = ResourceId.newFromContractAddress(
		vAnchor.getAddress(),
		ChainType.EVM,
		localChain.evmId
	);
	const functionSignature = hexToU8a(
		vAnchor.contract.interface.getSighash(
			'configureMaximumDepositLimit(uint256,uint32)'
		)
	);
	const nonce = Number(await vAnchor.contract.getProposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(
		resourceId,
		functionSignature,
		nonce
	);
	const maxLimitProposal = new MaxDepositLimitProposal(
		proposalHeader,
		'0x50000000'
	);

	// register proposal resourceId.
	await registerResourceId(polkadotApi, maxLimitProposal.header.resourceId);
	const prop = u8aToHex(maxLimitProposal.toU8a());

	const maxDepositLimitProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'MaxDepositLimitUpdate',
				data: prop,
			},
		}
	);

	const proposalCall =
		polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
			maxDepositLimitProposal.toU8a()
		);
	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalBatchSigned');

	const signedBatchProposals = await polkadotApi.query.dkgProposalHandler.signedProposals.entries();

	let signedDkgProposal = null;

	for (const proposalBatch of signedBatchProposals) {
		let proposals = JSON.parse(proposalBatch[1].toString())['proposals'];
		for (const proposal of proposals) {
			if (proposal.signed.kind == "MaxDepositLimitUpdate") {
				signedDkgProposal = proposal.signed;
				break;
			}
		}
	}

	console.log(signedDkgProposal);

	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureVBridge.getVBridgeSide(
		localChain.typedChainId
	);
	const contract = bridgeSide.contract;

	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		signedDkgProposal.data,
		signedDkgProposal.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const tx2 = await contract.executeProposalWithSignature(
		signedDkgProposal.data,
		signedDkgProposal.signature
	);
	await tx2.wait();
	// Want to check that fee was updated
	const maxDepositLimit = await vAnchor.contract.maximumDepositAmount();
	expect(hexToNumber('0x50000000').toString()).to.eq(
		maxDepositLimit.toString()
	);
});
