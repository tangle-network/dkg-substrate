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
import { FungibleTokenWrapper } from '@webb-tools/tokens';
import { hexToNumber, hexToU8a, u8aToHex } from '@polkadot/util';
import {
	WrappingFeeUpdateProposal,
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

it('should be able to sign wrapping fee update proposal', async () => {
	const anchor = signatureVBridge.getVAnchor(localChain.typedChainId)!;
	const governedTokenAddress = anchor.token!;
	let governedToken = FungibleTokenWrapper.connect(
		governedTokenAddress,
		wallet1
	);
	const resourceId = ResourceId.newFromContractAddress(
		governedTokenAddress,
		ChainType.EVM,
		localChain.evmId
	);
	const functionSig = hexToU8a(
		governedToken.contract.interface.getSighash('setFee(uint16,uint32)')
	);
	const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(resourceId, functionSig, nonce);

	const wrappingFeeProposal = new WrappingFeeUpdateProposal(
		proposalHeader,
		'0x50'
	);
	// register proposal resourceId.
	await registerResourceId(polkadotApi, wrappingFeeProposal.header.resourceId);
	const prop = u8aToHex(wrappingFeeProposal.toU8a());
	const wrappingFeeUpdateProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'WrappingFeeUpdate',
				data: prop,
			},
		}
	);
	const proposalCall =
		polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
			wrappingFeeUpdateProposal.toU8a()
		);

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalBatchSigned');

	// now we need to query the proposal and its signature.
	const signedBatchProposals = await polkadotApi.query.dkgProposalHandler.signedProposals.entries();

	let dkgProposal = null;

	for (const proposalBatch of signedBatchProposals) {
		let proposals = JSON.parse(proposalBatch[1].toString())['proposals'];
		for (const proposal of proposals) {
			if (proposal.signed.kind == "WrappingFeeUpdate" && proposal.signed.data == prop) {
				dkgProposal = proposal.signed;
				break;
			}
		}
	}

	console.log(dkgProposal);
	
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
	const fee = await governedToken.contract.getFee();
	expect(hexToNumber('0x50')).to.eq(fee);
});
