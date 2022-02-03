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

const resourceId = makeResourceId(
	'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
	ChainIdType.EVM,
	5002
);
let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;
const anchorUpdateProposal: AnchorUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	srcChainId: 5001,
	lastLeafIndex: 0,
	merkleRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
};

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
	const unsubSignedProps: any = await api.query.dKGProposalHandler.signedProposals(
		chainIdType,
		{ anchorupdateproposal: nonce },
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed anchor prop: ${JSON.stringify(parsedResult)}`);

				if (parsedResult) {
					const sig = parsedResult.anchorUpdateSigned.signature;
					console.log(`Signature: ${sig}`);

					const propHash = keccak256(encodeUpdateAnchorProposal(anchorUpdateProposal));
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

async function registerResourceId(api: ApiPromise) {
	// quick check if the resourceId is already registered
	const res = await api.query.dKGProposals.resources(resourceId);
	const val = new Option(api.registry, Bytes, res);
	if (val.isSome) {
		console.log(`Resource id ${resourceId} is already registered, skipping`);
		return;
	}
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');

	const call = api.tx.dKGProposals.setResource(resourceId, '0x00');
	console.log('Registering resource id');
	const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, ({ events = [], status }) => {
		console.log(`Current status is: ${status.type}`);

		if (status.isFinalized) {
			console.log(`Transaction included at blockHash ${status.asFinalized}`);

			events.forEach(({ phase, event: { data, method, section } }) => {
				console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
			});

			unsub();
		}
	});
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

	const unsub = await proposalCall.signAndSend(alice, ({ events = [], status }) => {
		console.log(`Current status is: ${status.type}`);

		if (status.isFinalized) {
			console.log(`Transaction included at blockHash ${status.asFinalized}`);

			events.forEach(({ phase, event: { data, method, section } }) => {
				console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
			});

			unsub();
		}
	});
}

// Run
testAnchorProposal()
	.catch(console.error)
	.finally(() => process.exit());
