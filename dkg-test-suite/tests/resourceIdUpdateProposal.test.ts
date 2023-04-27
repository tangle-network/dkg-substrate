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
import { sudoTx, waitForEvent } from './utils/setup';
import { FungibleTokenWrapper } from '@webb-tools/tokens';
import { hexToU8a, u8aToHex } from '@polkadot/util';
import {
	ResourceIdUpdateProposal,
	ChainType,
	ResourceId,
	ProposalHeader,
} from '@webb-tools/sdk-core';
import {
	localChain,
	polkadotApi,
	signatureVBridge,
	wallet1,
} from './utils/util';
import { expect } from 'chai';
import { registerResourceId } from '@webb-tools/test-utils';

it('should be able to sign resource id update proposal', async () => {
	const bridgeSide = signatureVBridge.getVBridgeSide(localChain.typedChainId);
	const resourceId = ResourceId.newFromContractAddress(
		bridgeSide.contract.address,
		ChainType.EVM,
		localChain.evmId
	);
	const proposalFunctionSignature = hexToU8a(
		bridgeSide.contract.interface.getSighash(
			'adminSetResourceWithSignature(bytes32,bytes4,uint32,bytes32,address,bytes)'
		)
	);

	const proposalNonce = Number(await bridgeSide.contract.proposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(
		resourceId,
		proposalFunctionSignature,
		proposalNonce
	);

	// Let's create a new FungibleTokenWrapper and set the resourceId for it via
	// the ResourceIdUpdate Proposal
	const dummyAddress = '0x1111111111111111111111111111111111111111';
	const governedToken = await FungibleTokenWrapper.createFungibleTokenWrapper(
		'token-e2e-test',
		'te2e',
		0,
		dummyAddress,
		dummyAddress,
		'10000000000000000000000000',
		false,
		wallet1
	);

	const newResourceId = ResourceId.newFromContractAddress(
		governedToken.contract.address,
		ChainType.EVM,
		localChain.evmId
	);
	const handlerAddress = bridgeSide.tokenHandler.contract.address;

	const resourceUpdateProposal = new ResourceIdUpdateProposal(
		proposalHeader,
		newResourceId.toString(),
		handlerAddress
	);

	// register proposal resourceId.
	await registerResourceId(
		polkadotApi,
		resourceUpdateProposal.header.resourceId
	);
	const prop = u8aToHex(resourceUpdateProposal.toU8a());

	const resourceIdUpdateProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'ResourceIdUpdate',
				data: prop,
			},
		}
	);
	const proposalCall =
		polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
			resourceIdUpdateProposal.toU8a()
		);

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
		key: 'resourceIdUpdateProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		ResourceIdUpdateProposal: resourceUpdateProposal.header.nonce,
	};
	const proposal = await polkadotApi.query.dkgProposalHandler.signedProposals(
		{
			Evm: localChain.evmId,
		},
		key
	);

	const dkgProposal = proposal.unwrap().asSigned;

	// sanity check.
	expect(u8aToHex(dkgProposal.data)).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const contract = bridgeSide.contract;
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.data,
		dkgProposal.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const tx2 = await contract.adminSetResourceWithSignature(
		resourceId.toString(),
		proposalFunctionSignature,
		proposalNonce,
		newResourceId.toString(),
		handlerAddress,
		dkgProposal.signature
	);
	await tx2.wait();

	expect(
		await bridgeSide.contract._resourceIdToHandlerAddress(
			newResourceId.toString()
		)
	).to.eq(handlerAddress);
});
