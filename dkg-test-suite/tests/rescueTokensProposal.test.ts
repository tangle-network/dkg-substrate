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
import { waitForEvent, sudoTx, sleep } from './utils/setup';
import { BigNumber } from 'ethers';
import {
	Treasury,
	GovernedTokenWrapper,
	MintableToken,
} from '@webb-tools/tokens';
import { hexToU8a, u8aToHex } from '@polkadot/util';
import {
	RescueTokensProposal,
	ChainType,
	WrappingFeeUpdateProposal,
	ResourceId,
	ProposalHeader,
	CircomUtxo,
} from '@webb-tools/sdk-core';
import {
	localChain,
	polkadotApi,
	signatureVBridge,
	wallet1,
} from './utils/util';
import { BLOCK_TIME } from './utils/constants';
import { expect } from 'chai';

it('should be able to sign and execute rescue token proposal', async () => {
	const anchor = signatureVBridge.getVAnchor(
		localChain.typedChainId,
	)!;
	const governedTokenAddress = anchor.token!;
	const governedToken = GovernedTokenWrapper.connect(
		governedTokenAddress,
		wallet1
	);
	const mintableTokenAddress = (await governedToken.contract.getTokens())[0];
	const mintableToken = await MintableToken.tokenFromAddress(
		mintableTokenAddress,
		wallet1
	);
	const treasuryAddress = await governedToken.getFeeRecipientAddress();
	const treasury = Treasury.connect(treasuryAddress, wallet1);

	// First, we will execute the update wrapping fee proposal to change the fee to be greater than 0
	// This will allow tokens to accumulate to the treasury
	{
		const governedTokenResourceId = ResourceId.newFromContractAddress(governedToken.contract.address, ChainType.EVM, localChain.evmId);
		const functionSignature = hexToU8a(governedToken.contract.interface.getSighash('setFee(uint16,uint32)'));
		const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
		const proposalHeader = new ProposalHeader(governedTokenResourceId, functionSignature, nonce);
		const wrappingFeeProposal = new WrappingFeeUpdateProposal(proposalHeader, '0x0A');

		const prop = u8aToHex(wrappingFeeProposal.toU8a());

		const wrappingFeeProposalType = polkadotApi.createType(
			'WebbProposalsProposal',
			{
				Unsigned: {
					kind: 'WrappingFeeUpdate',
					data: prop,
				},
			}
		);

		const proposalCall = polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(wrappingFeeProposalType.toU8a());

		await sudoTx(polkadotApi, proposalCall);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
			key: 'wrappingFeeUpdateProposal',
		});

		// now we need to query the proposal and its signature.
		const key = {
			WrappingFeeUpdateProposal: wrappingFeeProposal.header.nonce,
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
		const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
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
		expect(10).to.eq(fee);
	}
	await sleep(5 * BLOCK_TIME); // wait for a few blocks
	{
		// Now we wrap and deposit, the wrapping fee should accumulate to the treasury
		const wrappingFee = await governedToken.contract.getFee();
		await governedToken.grantMinterRole(anchor.contract.address);
		let tx = await mintableToken.approveSpending(anchor.contract.address);
		await tx.wait();
		tx = await mintableToken.approveSpending(governedToken.contract.address);
		await tx.wait();
		await mintableToken.mintTokens(wallet1.address, '100000000000000000000000');
		const outputUtxo = await CircomUtxo.generateUtxo({
			amount: '100000000000000',
			backend: 'Circom',
			chainId: localChain.typedChainId.toString(),
			curve: 'Bn254',
		});
		await anchor.transactWrap(
			mintableToken.contract.address,
			[],
			[outputUtxo],
			0,
			wallet1.address,
			wallet1.address,
			{}
		);

		// Anchor Denomination amount should go to TokenWrapper
		expect(
			(
				await mintableToken.getBalance(governedToken.contract.address)
			).toString()
		).to.eq(outputUtxo.amount);

		// The wrapping fee should be transferred to the treasury
		expect(
			(await mintableToken.getBalance(treasury.contract.address)).toString()
		).to.eq(
			BigNumber.from(outputUtxo.amount)
				.mul(wrappingFee)
				.div(10000 - wrappingFee)
				.toString()
		);

		expect(
			(
				await governedToken.contract.balanceOf(anchor.contract.address)
			).toString()
		).to.eq(outputUtxo.amount);
	}

	await sleep(5 * BLOCK_TIME); // wait for a few blocks

	// We now execute the rescue tokens proposal
	{
		const to = wallet1.address;
		let balTreasuryBeforeRescue = await mintableToken.getBalance(
			treasury.contract.address
		);
		let balToBeforeRescue = await mintableToken.getBalance(to);

		const treasuryResourceId =  ResourceId.newFromContractAddress(treasury.contract.address, ChainType.EVM, localChain.evmId);
		const functionSignature = hexToU8a(treasury.contract.interface.getSighash('rescueTokens(address,address,uint256,uint32)'));
		const nonce = Number(await treasury.contract.proposalNonce()) + 1
		const proposalHeader = new ProposalHeader(treasuryResourceId, functionSignature, nonce);
		const rescueTokensProposal = new RescueTokensProposal(proposalHeader, mintableTokenAddress, to, '0x01F4');

		const prop = u8aToHex(rescueTokensProposal.toU8a());
		const rescueTokensProposalType = polkadotApi.createType(
			'WebbProposalsProposal',
			{
				Unsigned: {
					kind: 'RescueTokens',
					data: prop,
				},
			}
		);
		const proposalCall =
			polkadotApi.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
				rescueTokensProposalType.toU8a()
			);

		await sudoTx(polkadotApi, proposalCall);

		console.log('after rescue sudo tx');
		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dkgProposalHandler', 'ProposalSigned', {
			key: 'rescueTokensProposal',
		});
		// now we need to query the proposal and its signature.
		const key = {
			RescueTokensProposal: rescueTokensProposal.header.nonce,
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
		const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
		const contract = bridgeSide.contract;
		console.log(await contract.governor());
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

		// We check that the address that the tokens were rescued to contain the rescued tokens
		let balTreasuryAfterRescue = await mintableToken.getBalance(
			treasury.contract.address
		);
		let balToAfterRescue = await mintableToken.getBalance(to);

		const diffTreasuryBalance = parseInt(
			balTreasuryBeforeRescue.sub(balTreasuryAfterRescue).toString()
		);
		const diffToBalance = parseInt(
			balToAfterRescue.sub(balToBeforeRescue).toString()
		);

		expect(500 == diffTreasuryBalance).to.eq(true);
		expect(500 == diffToBalance).to.eq(true);
	}
});
