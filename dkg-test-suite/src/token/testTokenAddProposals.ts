import {ApiPromise} from '@polkadot/api';
import {Keyring} from '@polkadot/keyring';
import {
	encodeTokenAddProposal,
	provider,
	waitNfinalizedBlocks,
} from '../util/utils';
import {ethers} from 'ethers';
import {keccak256} from '@ethersproject/keccak256';
import {ECPair} from 'ecpair';
import {assert, u8aToHex} from '@polkadot/util';
import {registerResourceId, resourceId} from "../util/resource";
import {tokenAddProposal} from "../util/proposals";

async function testTokenAddProposal() {
	const api = await ApiPromise.create({provider});
	await registerResourceId(api);
	await sendTokenAddProposal(api);
	console.log('Waiting for the DKG to Sign the proposal');
	await waitNfinalizedBlocks(api, 8, 20 * 7);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{compressed: false}
	).publicKey.toString('hex');
	const chainIdType = api.createType('DkgRuntimePrimitivesChainIdType', {EVM: 5002});
	const unsubSignedProps: any = await api.query.dKGProposalHandler.signedProposals(
		chainIdType,
		{tokenaddproposal: tokenAddProposal.header.nonce},
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed token add prop: ${JSON.stringify(parsedResult)}`);

				if (parsedResult) {
					const sig = parsedResult.anchorUpdateSigned.signature;
					console.log(`Signature: ${sig}`);

					const propHash = keccak256(encodeTokenAddProposal(tokenAddProposal));
					const recoveredPubKey = ethers.utils.recoverPublicKey(propHash, sig).substr(2);
					console.log(`Recovered public key: ${recoveredPubKey}`);
					console.log(`DKG public key: ${dkgPubKey}`);

					assert(recoveredPubKey == dkgPubKey, 'Public keys should match');
					if (recoveredPubKey == dkgPubKey) {
						console.log(`Public keys match`);
						process.exit(0);
					} else {
						console.error(`Public keys do not match`);
						process.exit(-1);
					}
				}
			}
		}
	);

	await new Promise((resolve) => setTimeout(resolve, 20000));

	unsubSignedProps();
}

async function sendTokenAddProposal(api: ApiPromise) {
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');

	const [authoritySetId, dkgPubKey] = await Promise.all([
		api.query.dkg.authoritySetId(),
		api.query.dkg.dKGPublicKey(),
	]);

	const prop = u8aToHex(encodeTokenAddProposal(tokenAddProposal));
	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);
	console.log(`Resource id is: ${resourceId}`);
	console.log(`Proposal is: ${prop}`);
	const chainIdType = api.createType('DkgRuntimePrimitivesChainIdType', {EVM: 5001});
	const kind = api.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenAdd');
	const proposal = api.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop
		}
	});
	const proposalCall = api.tx.dKGProposalHandler.forceSubmitUnsignedProposal(proposal);

	const unsub = await api.tx.sudo.sudo(proposalCall).signAndSend(alice, ({events = [], status}) => {
		console.log(`Current status is: ${status.type}`);

		if (status.isFinalized) {
			console.log(`Transaction included at blockHash ${status.asFinalized}`);

			events.forEach(({phase, event: {data, method, section}}) => {
				console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
			});

			unsub();
		}
	});
}

// Run
testTokenAddProposal()
	.catch(console.error)
	.finally(() => process.exit());
