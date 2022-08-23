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
	waitForEvent,
	sudoTx,
} from './utils/setup';
import { ethers } from 'ethers';
import { GovernedTokenWrapper } from '@webb-tools/tokens';
import { Keyring } from '@polkadot/api';
import { hexToNumber, hexToU8a, u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
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
	const anchor = signatureVBridge.getVAnchor(
		localChain.typedChainId,
	)!;
	const governedTokenAddress = anchor.token!;
	let governedToken = GovernedTokenWrapper.connect(
		governedTokenAddress,
		wallet1
	);
	const resourceId = ResourceId.newFromContractAddress(governedTokenAddress, ChainType.EVM, localChain.evmId);
	const functionSig = hexToU8a(governedToken.contract.interface.functions['setFee(uint16,uint32)'].format());
	const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(resourceId, functionSig, nonce);

	// Create Mintable Token to add to GovernedTokenWrapper
	//Create an ERC20 Token
	const wrappingFeeProposal = new WrappingFeeUpdateProposal(proposalHeader, '0x50');
	// register proposal resourceId.
	await registerResourceId(polkadotApi, wrappingFeeProposal.header.resourceId);
	const proposalBytes = wrappingFeeProposal.toU8a();
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
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
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
	expect(hexToNumber('0x50')).to.eq(fee);
});
