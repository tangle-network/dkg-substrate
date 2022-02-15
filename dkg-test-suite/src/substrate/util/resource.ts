import {ApiPromise} from "@polkadot/api";
import {Bytes, Option} from "@polkadot/types";
import {KeyringPair} from "@polkadot/keyring/types";
import {Keyring} from "@polkadot/keyring";
import {ChainIdType, makeResourceId} from "../utils";
import {ethers} from "ethers";
import {assert} from "@polkadot/util";

export const resourceId = makeResourceId(
	ChainIdType.SUBSTRATE,
	5002,
);

export async function signAndSendUtil(api: ApiPromise, proposalCall: any, alice: KeyringPair) {
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

export async function unsubSignedPropsUtil(api: ApiPromise, chainIdType: any, dkgPubKey: any, proposalType: any, propHash: any) {
	return await api.query.dKGProposalHandler.signedProposals(
		chainIdType,
		proposalType,
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed ${JSON.stringify(proposalType)} prop: ${JSON.stringify(parsedResult)}`);

				if (parsedResult) {
					const sig = parsedResult.signed.signature;
					console.log(`Signature: ${sig}`);

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
}

export async function registerResourceId(api: ApiPromise) {
	// quick check if the resourceId is already registered
	const res = await api.query.dKGProposals.resources(resourceId);
	const val = new Option(api.registry, Bytes, res);
	if (val.isSome) {
		console.log(`Resource id ${resourceId} is already registered, skipping`);
		return;
	}
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');

	const call = api.tx.dKGProposals.setResource(resourceId, '0x00');
	console.log('Registering resource id');
	const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, ({events = [], status}) => {
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

