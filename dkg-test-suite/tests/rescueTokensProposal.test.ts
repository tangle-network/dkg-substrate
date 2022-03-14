import 'jest-extended';
import { encodeFunctionSignature, registerResourceId, waitForEvent, sleep } from '../src/utils';
import { ethers, BigNumber } from 'ethers';
import { TreasuryHandler, Treasury, GovernedTokenWrapper, MintableToken } from '@webb-tools/tokens';
import { Keyring } from '@polkadot/api';
import { u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import {
	signAndSendUtil,
	RescueTokensProposal,
	ChainIdType,
	encodeRescueTokensProposal,
	WrappingFeeUpdateProposal,
	encodeWrappingFeeUpdateProposal,
} from '../src/evm/util/utils';
import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	executeAfter,
	localChain2,
	wallet2,
} from './utils/util';
import { SignatureBridgeSide } from '@webb-tools/bridges';
import { DeployerConfig } from '@webb-tools/interfaces';
import { BLOCK_TIME } from '../src/constants';
import { Anchors } from '@webb-tools/protocol-solidity';

describe('Rescue Token Proposal', () => {
	test('should be able to sign and execute rescue token proposal', async () => {
		const anchor = signatureBridge.getAnchor(
			localChain.chainId,
			ethers.utils.parseEther('1')
		)! as Anchors.Anchor;
		const governedTokenAddress = anchor.token!;
		const governedToken = GovernedTokenWrapper.connect(governedTokenAddress, wallet1);
		const mintableTokenAddress = (await governedToken.contract.getTokens())[0];
		const mintableToken = await MintableToken.tokenFromAddress(mintableTokenAddress, wallet1);
		const treasuryAddress = await governedToken.getFeeRecipientAddress();
		const treasury = Treasury.connect(treasuryAddress, wallet1);
		const keyring = new Keyring({ type: 'sr25519' });
		const alice = keyring.addFromUri('//Alice');
		const chainType = polkadotApi.createType('WebbProposalsHeaderChainType', 'Evm');
		// First, we will execute the update wrapping fee proposal to change the fee to be greater than 0
		// This will allow tokens to accumulate to the treasury
		{
			const governedTokenResourceId = await governedToken.createResourceId();
			// Create Mintable Token to add to GovernedTokenWrapper
			//Create an ERC20 Token
			const proposalPayload: WrappingFeeUpdateProposal = {
				header: {
					resourceId: governedTokenResourceId,
					functionSignature: encodeFunctionSignature(
						governedToken.contract.interface.functions['setFee(uint8,uint256)'].format()
					),
					nonce: Number(await governedToken.contract.proposalNonce()) + 1,
					chainIdType: ChainIdType.EVM,
					chainId: localChain.chainId,
				},
				newFee: '0x0A', // wrapping fee of 10 percent
			};

			const proposalBytes = encodeWrappingFeeUpdateProposal(proposalPayload);
			const prop = u8aToHex(proposalBytes);
			const kind = polkadotApi.createType(
				'DkgRuntimePrimitivesProposalProposalKind',
				'WrappingFeeUpdate'
			);
			const wrappingFeeUpdateProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
				Unsigned: {
					kind: kind,
					data: prop,
				},
			});
			const proposalCall =
				polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(wrappingFeeUpdateProposal);

			await signAndSendUtil(polkadotApi, proposalCall, alice);

			// now we need to wait until the proposal to be signed on chain.
			await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
			// now we need to query the proposal and its signature.
			const key = {
				WrappingFeeUpdateProposal: proposalPayload.header.nonce,
			};
			const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(
				[chainType, localChain.chainId],
				key
			);
			const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
			expect(value.isSome).toBeTrue();
			const dkgProposal = value.unwrap().toJSON() as {
				signed: {
					kind: 'WrappingFeeUpdate';
					data: HexString;
					signature: HexString;
				};
			};
			// sanity check.
			expect(dkgProposal.signed.data).toEqual(prop);
			// perfect! now we need to send it to the signature bridge.
			const bridgeSide = signatureBridge.getBridgeSide(localChain.chainId);
			const contract = bridgeSide.contract;
			const isSignedByGovernor = await contract.isSignatureFromGovernor(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			expect(isSignedByGovernor).toBeTrue();
			// check that we have the resouceId mapping.
			const tx2 = await contract.executeProposalWithSignature(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			await expect(tx2.wait()).toResolve();
			// Want to check that fee was updated
			const fee = await governedToken.contract.getFee();
			expect(10).toEqual(fee);
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
			expect((await mintableToken.getBalance(governedToken.contract.address)).toString()).toEqual(
				anchor.denomination!
			);

			// The wrapping fee should be transferred to the treasury
			expect((await mintableToken.getBalance(treasury.contract.address)).toString()).toEqual(
				BigNumber.from(anchor.denomination!)
					.mul(wrappingFee)
					.div(100 - wrappingFee)
					.toString()
			);

			expect((await governedToken.contract.balanceOf(anchor.contract.address)).toString()).toEqual(
				anchor.denomination!
			);
		}

		await sleep(5 * BLOCK_TIME); // wait for a few blocks

		// We now execute the rescue tokens proposal
		{
			const treasuryResourceId = await treasury.createResourceId();
			const to = wallet1.address;
			let balTreasuryBeforeRescue = await mintableToken.getBalance(treasury.contract.address);
			let balToBeforeRescue = await mintableToken.getBalance(to);
			const proposalPayload: RescueTokensProposal = {
				header: {
					resourceId: treasuryResourceId,
					functionSignature: encodeFunctionSignature(
						treasury.contract.interface.functions[
							'rescueTokens(address,address,uint256,uint256)'
						].format()
					),
					nonce: Number(await treasury.contract.proposalNonce()) + 1,
					chainIdType: ChainIdType.EVM,
					chainId: localChain.chainId,
				},
				tokenAddress: mintableTokenAddress,
				toAddress: to,
				amount: '0x01F4', // 500 in hex
			};
			const proposalBytes = encodeRescueTokensProposal(proposalPayload);
			const prop = u8aToHex(proposalBytes);
			const kind = polkadotApi.createType(
				'DkgRuntimePrimitivesProposalProposalKind',
				'RescueTokens'
			);
			const rescueTokensProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
				Unsigned: {
					kind: kind,
					data: prop,
				},
			});
			const proposalCall =
				polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(rescueTokensProposal);

			await signAndSendUtil(polkadotApi, proposalCall, alice);

			// now we need to wait until the proposal to be signed on chain.
			await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
			// now we need to query the proposal and its signature.
			const key = {
				RescueTokensProposal: proposalPayload.header.nonce,
			};
			const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(
				[chainType, localChain.chainId],
				key
			);
			const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
			expect(value.isSome).toBeTrue();
			const dkgProposal = value.unwrap().toJSON() as {
				signed: {
					kind: 'RescueTokens';
					data: HexString;
					signature: HexString;
				};
			};
			// sanity check.
			expect(dkgProposal.signed.data).toEqual(prop);
			// perfect! now we need to send it to the signature bridge.
			const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
			const contract = bridgeSide.contract;
			const isSignedByGovernor = await contract.isSignatureFromGovernor(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			expect(isSignedByGovernor).toBeTrue();
			// check that we have the resouceId mapping.
			const tx2 = await contract.executeProposalWithSignature(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			await expect(tx2.wait()).toResolve();

			// We check that the address that the tokens were rescued to contain the rescued tokens
			let balTreasuryAfterRescue = await mintableToken.getBalance(treasury.contract.address);
			let balToAfterRescue = await mintableToken.getBalance(to);

			const diffTreasuryBalance = parseInt(
				balTreasuryBeforeRescue.sub(balTreasuryAfterRescue).toString()
			);
			const diffToBalance = parseInt(balToAfterRescue.sub(balToBeforeRescue).toString());

			expect(500 == diffTreasuryBalance).toBeTrue();
			expect(500 == diffToBalance).toBeTrue();
		}
	});

	afterAll(async () => {
		await executeAfter();
	});
});
