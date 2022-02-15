import { ApiPromise } from '@polkadot/api';
import { Option, Bytes } from '@polkadot/types';
import { Keyring } from '@polkadot/keyring';
import {
	AnchorUpdateProposal,
	ChainIdType,
	encodeUpdateAnchorProposal,
	hexToBytes,
	makeResourceId,
	provider,
	waitNfinalizedBlocks,
} from './utils';
import { ethers } from 'ethers';
import { keccak256 } from '@ethersproject/keccak256';
import { ECPair } from 'ecpair';
import { assert, u8aToHex } from '@polkadot/util';
import {
	registerResourceId,
	resourceId,
	signAndSendUtil,
	unsubSignedPropsUtil
} from "./util/resource";
import {anchorUpdateProposal} from "./util/proposals";

async function testAnchorProposal() {
	const api = await ApiPromise.create({ provider });
	await registerResourceId(api);
	await sendAnchorProposal(api);
	console.log('Waiting for the DKG to Sign the proposal');
	await waitNfinalizedBlocks(api, 8, 20 * 7);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{ compressed: false }
	).publicKey.toString('hex');
	const chainIdType = api.createType('DkgRuntimePrimitivesChainIdType', { EVM: 5002 });

	const propHash = keccak256(encodeUpdateAnchorProposal(anchorUpdateProposal));

	const proposalType = { anchorupdateproposal: anchorUpdateProposal.header.nonce }

	const unsubSignedProps: any = await unsubSignedPropsUtil(api, chainIdType, dkgPubKey, proposalType, propHash);

	await new Promise((resolve) => setTimeout(resolve, 20000));

	unsubSignedProps();
}

async function sendAnchorProposal(api: ApiPromise) {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');

	const [authoritySetId, dkgPubKey] = await Promise.all([
		api.query.dkg.authoritySetId(),
		api.query.dkg.dKGPublicKey(),
	]);

	const prop = u8aToHex(encodeUpdateAnchorProposal(anchorUpdateProposal));
	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);
	console.log(`Resource id is: ${resourceId}`);
	console.log(`Proposal is: ${prop}`);
	const chainIdType = api.createType('DkgRuntimePrimitivesChainIdType', { EVM: 5001 });
	const proposalCall = api.tx.dKGProposals.acknowledgeProposal(0, chainIdType, resourceId, prop);

	await signAndSendUtil(api, proposalCall, alice);
}

// Run
testAnchorProposal()
	.catch(console.error)
	.finally(() => process.exit());
