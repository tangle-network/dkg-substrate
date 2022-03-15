import {ApiPromise} from '@polkadot/api';
import {Keyring} from '@polkadot/keyring';
import {
	ChainIdType,
	encodeSubstrateProposal, 
	makeResourceId,
	registerResourceId, 
	signAndSendUtil, 
	unsubSignedPropsUtil,
	substratePalletResourceId,
} from './util/utils';
import {
	provider,
	waitNfinalizedBlocks,
} from '../utils';
import {ethers} from 'ethers';
import {keccak256} from '@ethersproject/keccak256';
import {ECPair} from 'ecpair';
import {assert, u8aToHex} from '@polkadot/util';
import {getAnchorUpdateProposal} from "./util/proposals";

async function testAnchorUpdateProposal() {
	const api = await ApiPromise.create({provider});
	await registerResourceId(api);
	await sendAnchorUpdateProposal(api);
	console.log('Waiting for the DKG to Sign the proposal');
	await waitNfinalizedBlocks(api, 8, 20 * 7);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{compressed: false}
	).publicKey.toString('hex');
	const chainIdType = api.createType('WebbProposalsHeaderTypedChainId', {SUBSTRATE: 5002});
	const propHash = keccak256(encodeSubstrateProposal(getAnchorUpdateProposal(api), 3000));

	const proposalType = {anchorupdateproposal: getAnchorUpdateProposal(api).header.nonce}

	const unsubSignedProps: any = await unsubSignedPropsUtil(api, chainIdType, dkgPubKey, proposalType, propHash);

	await new Promise((resolve) => setTimeout(resolve, 50000));

	unsubSignedProps();
}

async function sendAnchorUpdateProposal(api: ApiPromise) {
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');

	const [authoritySetId, dkgPubKey] = await Promise.all([
		api.query.dkg.authoritySetId(),
		api.query.dkg.dKGPublicKey(),
	]);

	const prop = u8aToHex(encodeSubstrateProposal(getAnchorUpdateProposal(api), 3000));
	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);
	console.log(`Resource id is: ${substratePalletResourceId}`);
	console.log(`Proposal is: ${prop}`);
	const chainIdType = api.createType('WebbProposalsHeaderTypedChainId', {SUBSTRATE: 5001});
	const kind = api.createType('DkgRuntimePrimitivesProposalProposalKind', 'AnchorUpdate');
	const proposal = api.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop
		}
	});
	const proposalCall = api.tx.dKGProposalHandler.forceSubmitUnsignedProposal(proposal);

	await signAndSendUtil(api, proposalCall, alice);
}

// Run
testAnchorUpdateProposal()
	.catch(console.error)
	.finally(() => process.exit());
