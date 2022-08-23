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
import { ethers, BigNumber } from 'ethers';
import {
	Treasury,
	GovernedTokenWrapper,
	MintableToken,
} from '@webb-tools/tokens';
import { Keyring } from '@polkadot/api';
import { hexToU8a, u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import {
	RescueTokensProposal,
	ChainType,
	WrappingFeeUpdateProposal,
	ResourceId,
	ProposalHeader,
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
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const chainIdType = polkadotApi.createType(
		'WebbProposalsHeaderTypedChainId',
		{
			Evm: localChain.evmId,
		}
	);
	// First, we will execute the update wrapping fee proposal to change the fee to be greater than 0
	// This will allow tokens to accumulate to the treasury
	{
		const governedTokenResourceId = ResourceId.newFromContractAddress(governedToken.contract.address, ChainType.EVM, localChain.evmId);
		const functionSignature = hexToU8a(governedToken.contract.interface.functions['setFee(uint16,uint32)'].format());
		const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
		const proposalHeader = new ProposalHeader(governedTokenResourceId, functionSignature, nonce);
		const wrappingFeeProposal = new WrappingFeeUpdateProposal(proposalHeader, '0x0A');

		const proposalBytes = wrappingFeeProposal.toU8a();
		const prop = u8aToHex(proposalBytes);
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
			polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(
				wrappingFeeUpdateProposal
			);

		await sudoTx(polkadotApi, proposalCall);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
			key: 'WrappingFeeUpdateProposal',
		});
		// now we need to query the proposal and its signature.
		const key = {
			WrappingFeeUpdateProposal: wrappingFeeProposal.header.nonce,
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
				kind: 'WrappingFeeUpdate';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).to.eq(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
		const contract = bridgeSide.contract;
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
		const fee = await governedToken.contract.getFee();
		expect(10).to.eq(fee);
	}
	await sleep(5 * BLOCK_TIME); // wait for a few blocks
	{
		// Now we wrap and deposit, the wrapping fee should accumulate to the treasury
		const wrappingFee = await governedToken.contract.getFee();
		await governedToken.grantMinterRole(anchor.contract.address);
		await mintableToken.approveSpending(anchor.contract.address);
		await mintableToken.approveSpending(governedToken.contract.address);
		await mintableToken.mintTokens(wallet1.address, '100000000000000000000000');
		await anchor.wrapAndDeposit(mintableToken.contract.address, wrappingFee);

		// Anchor Denomination amount should go to TokenWrapper
		expect(
			(
				await mintableToken.getBalance(governedToken.contract.address)
			).toString()
		).to.eq(anchor.denomination!);

		// The wrapping fee should be transferred to the treasury
		expect(
			(await mintableToken.getBalance(treasury.contract.address)).toString()
		).to.eq(
			BigNumber.from(anchor.denomination!)
				.mul(wrappingFee)
				.div(100 - wrappingFee)
				.toString()
		);

		expect(
			(
				await governedToken.contract.balanceOf(anchor.contract.address)
			).toString()
		).to.eq(anchor.denomination!);
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
		const functionSignature = hexToU8a(treasury.contract.interface.functions['rescueTokens(address,address,uint256,uint32)'].format())
		const nonce = await Number(treasury.contract.proposalNonce()) + 1
		const proposalHeader = new ProposalHeader(treasuryResourceId, functionSignature, nonce);
		const rescueTokensProposal = new RescueTokensProposal(proposalHeader, mintableTokenAddress, to, '0x01F4');

		const proposalBytes = rescueTokensProposal.toU8a();
		const prop = u8aToHex(proposalBytes);
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
			polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(
				rescueTokensProposalType
			);

		await sudoTx(polkadotApi, proposalCall);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
			key: 'RescueTokensProposal',
		});
		// now we need to query the proposal and its signature.
		const key = {
			RescueTokensProposal: rescueTokensProposal.header.nonce,
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
				kind: 'RescueTokens';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).to.eq(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
		const contract = bridgeSide.contract;
		console.log(await contract.governor());
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
