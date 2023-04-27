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
import { sleep, waitForEvent, sudoTx } from './utils/setup';
import { MintableToken, FungibleTokenWrapper } from '@webb-tools/tokens';
import { hexToU8a, u8aToHex } from '@polkadot/util';
import {
	TokenAddProposal,
	ChainType,
	TokenRemoveProposal,
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
import { BLOCK_TIME } from './utils/constants';
import { registerResourceId } from '@webb-tools/test-utils';

it('should be able to sign token add & remove proposal', async () => {
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
	const functionSignature = hexToU8a(
		governedToken.contract.interface.getSighash('add(address,uint32)')
	);
	const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(
		resourceId,
		functionSignature,
		nonce
	);
	// Create Mintable Token to add to FungibleTokenWrapper
	//Create an ERC20 Token
	const tokenToAdd = await MintableToken.createToken(
		'testToken',
		'TEST',
		wallet1
	);
	{
		const tokenAddProposal = new TokenAddProposal(
			proposalHeader,
			tokenToAdd.contract.address
		);

		// register proposal resourceId.
		await registerResourceId(polkadotApi, tokenAddProposal.header.resourceId);
		const prop = u8aToHex(tokenAddProposal.toU8a());
		const tokenAddProposalType = polkadotApi.createType(
			'WebbProposalsProposal',
			{
				Unsigned: {
					kind: 'TokenAdd',
					data: prop,
				},
			}
		);
		const proposalCall =
			polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
				tokenAddProposalType.toU8a()
			);

		await sudoTx(polkadotApi, proposalCall);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
			key: 'tokenAddProposal',
		});
		// now we need to query the proposal and its signature.
		const key = {
			TokenAddProposal: tokenAddProposal.header.nonce,
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
		// Want to check that token was actually added
		expect(
			(await governedToken.contract.getTokens()).includes(
				tokenToAdd.contract.address
			)
		).to.eq(true);
	}
	await sleep(5 * BLOCK_TIME);
	const removeSignature = hexToU8a(
		governedToken.contract.interface.getSighash('remove(address,uint32)')
	);
	const proposalNonce =
		Number(await governedToken.contract.proposalNonce()) + 1;
	const removeProposalHeader = new ProposalHeader(
		resourceId,
		removeSignature,
		proposalNonce
	);
	const tokenRemoveProposal = new TokenRemoveProposal(
		removeProposalHeader,
		tokenToAdd.contract.address
	);

	// register proposal resourceId.
	await registerResourceId(polkadotApi, tokenRemoveProposal.header.resourceId);
	const proposalBytes = tokenRemoveProposal.toU8a();
	// get alice account to send the transaction to the dkg node.
	const prop = u8aToHex(proposalBytes);

	const tokenRemoveProposalType = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'TokenRemove',
				data: prop,
			},
		}
	);
	const proposalCall =
		polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
			tokenRemoveProposalType.toU8a()
		);

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
		key: 'tokenRemoveProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		TokenRemoveProposal: tokenRemoveProposal.header.nonce,
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
	// Want to check that token was actually added
	expect(
		(await governedToken.contract.getTokens()).includes(
			tokenToAdd.contract.address
		)
	).to.eq(false);
});
