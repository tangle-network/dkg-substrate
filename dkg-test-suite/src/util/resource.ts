import {ApiPromise} from "@polkadot/api";
import {Bytes, Option} from "@polkadot/types";
import {Keyring} from "@polkadot/keyring";
import {ChainIdType, makeResourceId} from "./utils";

export const resourceId = makeResourceId(
	'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
	ChainIdType.EVM,
	5002
);

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
