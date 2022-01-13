import { ApiPromise } from '@polkadot/api';
import { Keyring } from '@polkadot/keyring';
import {
	AnchorUpdateProposal,
	encodeUpdateAnchorProposal,
	hexToBytes,
	makeResourceId,
	provider,
	waitNfinalizedBlocks,
} from './utils';
import { ethers } from 'ethers';
import { keccak256 } from '@ethersproject/keccak256';
import { ECPair } from 'ecpair';
import { assert } from '@polkadot/util';

const resourceId = makeResourceId('0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667', 5002);
const anchorUpdateProposal: AnchorUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce: 0,
	},
	srcChainId: 5001,
	lastLeafIndex: 0,
	merkleRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
};

async function testAnchorProposal() {
	const api = await ApiPromise.create({ provider });

	await sendAnchorProposal(api);

	await waitNfinalizedBlocks(api, 2, 20 * 5);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{ compressed: false }
	).publicKey.toString('hex');

	const unsubSignedProps: any = await api.query.dKGProposalHandler.signedProposals(
		5001,
		{ anchorupdateproposal: 0 },
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed anchor prop: ${res.toHuman()}`);

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
					} else {
						console.error(`Public keys do not match`);
						process.exit();
					}
				}
			}
		}
	);

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

	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);
	console.log(`Resource id is: ${resourceId}`);
	const proposalCall = api.tx.dKGProposals.acknowledgeProposal(
		0,
		5001,
		resourceId,
		encodeUpdateAnchorProposal(anchorUpdateProposal)
	);

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
