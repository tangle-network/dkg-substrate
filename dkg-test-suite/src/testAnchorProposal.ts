import { ApiPromise } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import { hexToBytes, provider, waitNfinalizedBlocks } from "./utils";
import { ethers } from "ethers";
import { keccak256 } from "@ethersproject/keccak256";
import { ECPair } from "ecpair";
import { assert } from "@polkadot/util";

const anchorUpdateProp = new Uint8Array([
	209, 1, 81, 230, 199, 165, 40, 225, 135, 229, 62, 97, 95, 108, 125, 232, 16, 159, 158, 30, 76,
	163, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	19, 137, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0, 23, 194, 192, 194, 63, 104, 94, 171, 45, 188, 176, 167, 116, 213, 48, 159, 72, 180, 30,
	153, 205, 53, 55, 38, 185, 131, 173, 100, 167, 96, 194, 18,
]);
const resourceId = hexToBytes("0000000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b6670000138a");

async function testAnchorProposal() {
	const api = await ApiPromise.create({ provider });

	await waitNfinalizedBlocks(api, 10, 20 * 5);

	await sendAnchorProposal(api);

	await waitNfinalizedBlocks(api, 20, 20 * 5);

	const dkgPubKeyCompressed: any = await api.query.dkg.dKGPublicKey();
	const dkgPubKey = ECPair.fromPublicKey(
		Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), "hex"),
		{ compressed: false }
	).publicKey.toString("hex");

	const unsubSignedProps: any = await api.query.dKGProposalHandler.signedProposals(
		5001,
		{ anchorupdateproposal: 0 },
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed anchor prop: ${parsedResult}`);

				if (parsedResult) {
					const sig = parsedResult.anchorUpdateSigned.signature;
					console.log(`Signature: ${sig}`);

					const propHash = keccak256(anchorUpdateProp);
					const recoveredPubKey = ethers.utils.recoverPublicKey(propHash, sig).substr(2);
					console.log(`Recovered public key: ${recoveredPubKey}`);
					console.log(`DKG public key: ${dkgPubKey}`);

					assert(recoveredPubKey == dkgPubKey, "Public keys should match");
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
	const keyring = new Keyring({ type: "sr25519" });
	const alice = keyring.addFromUri("//Alice");

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
