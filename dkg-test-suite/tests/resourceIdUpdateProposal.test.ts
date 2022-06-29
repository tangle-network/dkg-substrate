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
import { encodeFunctionSignature, registerResourceId, waitForEvent } from '../src/utils';
import { GovernedTokenWrapper } from '@webb-tools/tokens';
import { Keyring } from '@polkadot/api';
import { u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import {
	signAndSendUtil,
	ResourceIdUpdateProposal,
	encodeResourceIdUpdateProposal,
	ChainIdType,
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

it('should be able to sign resource id update proposal', async () => {
	const bridgeSide = signatureBridge.getBridgeSide(localChain.chainId);
	const resourceId = await bridgeSide.createResourceId();

	// Let's create a new GovernedTokenWrapper and set the resourceId for it via
	// the ResourceIdUpdate Proposal
	const dummyAddress = '0x1111111111111111111111111111111111111111';
	const governedToken = await GovernedTokenWrapper.createGovernedTokenWrapper(
		'token-e2e-test',
		'te2e',
		dummyAddress,
		dummyAddress,
		'10000000000000000000000000',
		false,
		wallet1
	);

	const newResourceId = await governedToken.createResourceId();
	const handlerAddress = bridgeSide.tokenHandler.contract.address;
	const executionContextAddress = governedToken.contract.address;

	const proposalFunctionSignature = encodeFunctionSignature(
		bridgeSide.contract.interface.functions[
			'adminSetResourceWithSignature(bytes32,bytes4,uint32,bytes32,address,address,bytes)'
		].format()
	);

	const proposalNonce = Number(await bridgeSide.contract.proposalNonce()) + 1;
	const proposalPayload: ResourceIdUpdateProposal = {
		header: {
			resourceId,
			functionSignature: proposalFunctionSignature,
			nonce: proposalNonce,
			chainIdType: ChainIdType.EVM,
			chainId: localChain.chainId,
		},
		newResourceId: newResourceId,
		handlerAddress: handlerAddress,
		executionAddress: executionContextAddress,
	};
	// register proposal resourceId.
	await registerResourceId(polkadotApi, proposalPayload.header.resourceId);
	const proposalBytes = encodeResourceIdUpdateProposal(proposalPayload);
	// get alice account to send the transaction to the dkg node.
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const prop = u8aToHex(proposalBytes);
	const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
		Evm: localChain.chainId,
	});
	const kind = polkadotApi.createType(
		'DkgRuntimePrimitivesProposalProposalKind',
		'ResourceIdUpdate'
	);
	const resourceIdUpdateProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop,
		},
	});
	const proposalCall =
		polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(resourceIdUpdateProposal);

	await signAndSendUtil(polkadotApi, proposalCall, alice);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
		key: 'ResourceIdUpdateProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		ResourceIdUpdateProposal: proposalPayload.header.nonce,
	};
	const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
	const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
	expect(value.isSome).to.eq(true);
	const dkgProposal = value.unwrap().toJSON() as {
		signed: {
			kind: 'ResourceIdUpdate';
			data: HexString;
			signature: HexString;
		};
	};
	// sanity check.
	expect(dkgProposal.signed.data).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const contract = bridgeSide.contract;
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.signed.data,
		dkgProposal.signed.signature
	);
	expect(isSignedByGovernor).to.eq(true);
	// check that we have the resouceId mapping.
	const tx2 = await contract.adminSetResourceWithSignature(
		resourceId,
		proposalFunctionSignature,
		proposalNonce,
		newResourceId,
		handlerAddress,
		executionContextAddress,
		dkgProposal.signed.signature
	);
	await tx2.wait();

	expect(await bridgeSide.contract._resourceIDToHandlerAddress(newResourceId)).to.eq(
		handlerAddress
	);
});
