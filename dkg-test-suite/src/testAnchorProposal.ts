import { ApiPromise } from '@polkadot/api';
import { Keyring } from '@polkadot/keyring';
import { hexToBytes, provider, waitNfinalizedBlocks } from './utils';
import { ethers } from 'ethers';
import { keccak256 } from '@ethersproject/keccak256';
import { ECPair } from 'ecpair';
import { assert } from '@polkadot/util';
import { Anchor } from '@webb-tools/fixed-bridge';

const anchorUpdateProp = new Uint8Array([
    0,   0,   0,   0,   0,   0,   0,   0, 223,  22, 158, 136,
  193,  21, 177, 236, 107,  47, 234, 158, 193, 108, 153,  64,
  171, 132,  14,   7,   0,   0,   5,  57,  68,  52, 123, 169,
    0,   0,   0,   1,   0,   0, 122, 105,   0,   0,   0,   0,
   37, 168,  34, 127, 179, 164,  10,  49, 149, 165, 172, 173,
  194, 178,  58,  98, 176,  16, 209,  39, 221, 166,  75, 249,
  181, 131, 238,  94,  88, 214, 203,  31
]);
const resourceId = hexToBytes('0000000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b6670000138a');

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
				console.log(res);
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed anchor prop: ${parsedResult}`);

				if (parsedResult) {
					const sig = parsedResult.anchorUpdateSigned.signature;
					console.log(`Signature: ${sig}`);

					const propHash = keccak256(anchorUpdateProp);
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

	// console.log(`tx ${Object.getOwnPropertyNames(api.tx.dKGProposals)}`);

	const proposalData = new Uint8Array(130);
	for (let i = 0; i < proposalData.length; i++) {
		proposalData[i] = i / 255;
	}

	console.log(`Resource id is: ${resourceId}`);

	const proposalCall = api.tx.dKGProposals.acknowledgeProposal(
		0,
		5001,
		resourceId,
		anchorUpdateProp
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
