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
import { ApiPromise } from '@polkadot/api';
import { Keyring } from '@polkadot/keyring';
import {
	encodeMaxDepositLimitProposal,
	registerResourceId,
	resourceId,
	signAndSendUtil,
	unsubSignedPropsUtil,
} from '../util/utils';
import { provider, waitNfinalizedBlocks } from '../../utils';
import { keccak256 } from '@ethersproject/keccak256';
import { ECPair } from 'ecpair';
import { assert, u8aToHex } from '@polkadot/util';
import { maxDepositLimitProposal } from '../util/proposals';

async function testMaxDepositLimitUpdateProposal() {
	const api = await ApiPromise.create({ provider });
	await registerResourceId(api);
	await sendMaxDepositLimitUpdateProposal(api);
	console.log('Waiting for the DKG to Sign the proposal');
	await waitNfinalizedBlocks(api, 8, 20 * 7);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{ compressed: false }
	).publicKey.toString('hex');
	const chainIdType = api.createType('WebbProposalsHeaderTypedChainId', { Evm: 5002 });
	const propHash = keccak256(encodeMaxDepositLimitProposal(maxDepositLimitProposal));

	const proposalType = { maxdepositlimitupdateproposal: maxDepositLimitProposal.header.nonce };

	const unsubSignedProps: any = await unsubSignedPropsUtil(
		api,
		chainIdType,
		dkgPubKey,
		proposalType,
		propHash
	);

	await new Promise((resolve) => setTimeout(resolve, 20000));

	unsubSignedProps();
}

async function sendMaxDepositLimitUpdateProposal(api: ApiPromise) {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');

	const [authoritySetId, dkgPubKey] = await Promise.all([
		api.query.dkg.authoritySetId(),
		api.query.dkg.dKGPublicKey(),
	]);

	const prop = u8aToHex(encodeMaxDepositLimitProposal(maxDepositLimitProposal));
	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);
	console.log(`Resource id is: ${resourceId}`);
	console.log(`Proposal is: ${prop}`);
	const chainIdType = api.createType('WebbProposalsHeaderTypedChainId', { Evm: 5001 });
	const kind = api.createType('DkgRuntimePrimitivesProposalProposalKind', 'MaxDepositLimitUpdate');
	const proposal = api.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop,
		},
	});
	const proposalCall = api.tx.dKGProposalHandler.forceSubmitUnsignedProposal(proposal);

	await signAndSendUtil(api, proposalCall, alice);
}

// Run
testMaxDepositLimitUpdateProposal()
	.catch(console.error)
	.finally(() => process.exit());
