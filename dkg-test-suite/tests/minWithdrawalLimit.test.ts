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
import { waitForEvent, sudoTx } from './utils/setup';
import { hexToNumber, u8aToHex, hexToU8a } from '@polkadot/util';
import {
	MinWithdrawalLimitProposal,
	ChainType,
	ResourceId,
	ProposalHeader,
} from '@webb-tools/sdk-core';
import { localChain, polkadotApi, signatureVBridge } from './utils/util';
import { expect } from 'chai';
import { registerResourceId } from '@webb-tools/test-utils';

it('should be able to update min withdrawal limit', async () => {
	const vAnchor = signatureVBridge.getVAnchor(localChain.typedChainId)!;
	const resourceId = ResourceId.newFromContractAddress(
		vAnchor.contract.address,
		ChainType.EVM,
		localChain.evmId
	);
	const functionSignature = hexToU8a(
		vAnchor.contract.interface.getSighash(
			'configureMinimalWithdrawalLimit(uint256,uint32)'
		)
	);
	const nonce = Number(await vAnchor.contract.getProposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(
		resourceId,
		functionSignature,
		nonce
	);
	const minLimitProposal = new MinWithdrawalLimitProposal(
		proposalHeader,
		'0x50'
	);

	// register proposal resourceId.
	await registerResourceId(polkadotApi, minLimitProposal.header.resourceId);

	// get alice account to send the transaction to the dkg node.
	const prop = u8aToHex(minLimitProposal.toU8a());
	const minWithdrawalLimitProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'MinWithdrawalLimitUpdate',
				data: prop,
			},
		}
	);

	const proposalCall =
		polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
			minWithdrawalLimitProposal.toU8a()
		);

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalBatchSigned');

	// now we need to query the proposal and its signature.
	const key = {
		MinWithdrawalLimitUpdateProposal: minLimitProposal.header.nonce,
	};
	const proposal = await polkadotApi.query.dkgProposalHandler.signedProposals(
		{
			Evm: localChain.evmId,
		},
		key
	);

	const dkgProposal = proposal.unwrap().asSigned;

	// sanity check.
	expect(dkgProposal.data.toString()).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureVBridge.getVBridgeSide(
		localChain.typedChainId
	);
	const contract = bridgeSide.contract;
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.data,
		dkgProposal.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const tx2 = await contract.executeProposalWithSignature(
		dkgProposal.data,
		dkgProposal.signature
	);
	await tx2.wait();
	// Want to check that fee was updated
	const minWithdrawalLimit = await vAnchor.contract.minimalWithdrawalAmount();
	expect(hexToNumber('0x50').toString()).to.eq(minWithdrawalLimit.toString());
});
