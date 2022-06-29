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
import { encodeFunctionSignature, registerResourceId, sleep, waitForEvent } from '../src/utils';
import { ethers } from 'ethers';
import { MintableToken, GovernedTokenWrapper } from '@webb-tools/tokens';
import { Keyring } from '@polkadot/api';
import { u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import {
	signAndSendUtil,
	TokenAddProposal,
	encodeTokenAddProposal,
	ChainIdType,
	encodeTokenRemoveProposal,
	TokenRemoveProposal,
} from '../src/evm/util/utils';
import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	executeAfter,
	executeBefore,
} from './utils/util';
import { Bridges } from '@webb-tools/protocol-solidity';
import { expect } from 'chai';
import { BLOCK_TIME } from '../src/constants';

it('should be able to sign token add & remove proposal', async () => {
	const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
	const governedTokenAddress = anchor.token!;
	let governedToken = GovernedTokenWrapper.connect(governedTokenAddress, wallet1);
	const resourceId = await governedToken.createResourceId();
	// Create Mintable Token to add to GovernedTokenWrapper
	//Create an ERC20 Token
	const tokenToAdd = await MintableToken.createToken('testToken', 'TEST', wallet1);
	{
		const proposalPayload: TokenAddProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					governedToken.contract.interface.functions['add(address,uint256)'].format()
				),
				nonce: Number(await governedToken.contract.proposalNonce()) + 1,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			newTokenAddress: tokenToAdd.contract.address,
		};
		// register proposal resourceId.
		await registerResourceId(polkadotApi, proposalPayload.header.resourceId);
		const proposalBytes = encodeTokenAddProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({ type: 'sr25519' });
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
			Evm: localChain.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenAdd');
		const tokenAddProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop,
			},
		});
		const proposalCall =
			polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(tokenAddProposal);

		await signAndSendUtil(polkadotApi, proposalCall, alice);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
			key: 'TokenAddProposal',
		});
		// now we need to query the proposal and its signature.
		const key = {
			TokenAddProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
		expect(value.isSome).to.eq(true);
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'TokenAdd';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).to.eq(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
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
		// Want to check that token was actually added
		expect((await governedToken.contract.getTokens()).includes(tokenToAdd.contract.address)).to.eq(
			true
		);
	}
	await sleep(5 * BLOCK_TIME);
	const proposalPayload: TokenRemoveProposal = {
		header: {
			resourceId,
			functionSignature: encodeFunctionSignature(
				governedToken.contract.interface.functions['remove(address,uint256)'].format()
			),
			nonce: Number(await governedToken.contract.proposalNonce()) + 1,
			chainIdType: ChainIdType.EVM,
			chainId: localChain.chainId,
		},
		removeTokenAddress: tokenToAdd.contract.address,
	};
	// register proposal resourceId.
	await registerResourceId(polkadotApi, proposalPayload.header.resourceId);
	const proposalBytes = encodeTokenRemoveProposal(proposalPayload);
	// get alice account to send the transaction to the dkg node.
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const prop = u8aToHex(proposalBytes);
	const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
		Evm: localChain.chainId,
	});
	const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenRemove');
	const tokenRemoveProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop,
		},
	});
	const proposalCall =
		polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(tokenRemoveProposal);

	await signAndSendUtil(polkadotApi, proposalCall, alice);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
		key: 'TokenRemoveProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		TokenRemoveProposal: proposalPayload.header.nonce,
	};
	const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
	const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);

	expect(value.isSome).to.eq(true);
	const dkgProposal = value.unwrap().toJSON() as {
		signed: {
			kind: 'TokenRemove';
			data: HexString;
			signature: HexString;
		};
	};
	// sanity check.
	expect(dkgProposal.signed.data).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
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
	// Want to check that token was actually added
	expect((await governedToken.contract.getTokens()).includes(tokenToAdd.contract.address)).to.eq(
		false
	);
});
