import { ApiPromise } from '@polkadot/api';
import { Keyring } from '@polkadot/keyring';
import { hexToBytes, listenOneBlock, provider, waitNfinalizedBlocks } from './utils';
import { ethers } from 'ethers';
import { keccak256 } from '@ethersproject/keccak256';
import { ECPair } from 'ecpair';
import { assert } from '@polkadot/util';
import { apiProposalTypes } from './proposalTypes';

const raw_data =
	'00000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000001';
const tokenUpdateProp = new Uint8Array(hexToBytes(raw_data));

async function testDirectProposal() {
	const api = await apiProposalTypes();

	await waitNfinalizedBlocks(api, 10, 20 * 5);

	await sendSudoProposal(api);

	await waitNfinalizedBlocks(api, 20, 20 * 5);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
		{ compressed: false }
	).publicKey.toString('hex');

	const unsubSignedProps: any = await api.query.dKGProposalHandler.signedProposals(
		1,
		{ tokenupdateproposal: 1 },
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed prop: ${parsedResult}`);
				assert(parsedResult, 'Signed proposal should be on chain');

				if (parsedResult) {
					const sig = parsedResult.tokenUpdateSigned.signature;
					console.log(`Signature: ${sig}`);

					const propHash = keccak256(tokenUpdateProp);
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

async function sendSudoProposal(api: ApiPromise) {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');

	await listenOneBlock(api);

	const [authoritySetId, dkgPubKey] = await Promise.all([
		api.query.dkg.authoritySetId(),
		api.query.dkg.dKGPublicKey(),
	]);

	console.log(`DKG authority set id: ${authoritySetId}`);
	console.log(`DKG pub key: ${dkgPubKey}`);

	const call = api.tx.dKGProposalHandler.forceSubmitUnsignedProposal({
		TokenUpdate: {
			data: `0x${raw_data}`,
		},
	});
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

// Run
testDirectProposal()
	.catch(console.error)
	.finally(() => process.exit());
