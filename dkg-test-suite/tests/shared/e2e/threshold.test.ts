/*
 * Copyright 2022 Webb Technologies Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
import {ApiPromise, Keyring} from "@polkadot/api";
import {
	waitForEvent,
	waitForTheNextSession
} from "../../../src/utils";

export async function thresholdTest(polkadotApi: ApiPromise) {
	const keyring = new Keyring({ type: 'sr25519' });
	const charlieStash = keyring.addFromUri('//Charlie//stash');
	const alice = keyring.addFromUri('//Alice');

	// check and expect threshold count to be 2
	let thresholdCount = await polkadotApi.query.dkg.signatureThreshold();
	expect(thresholdCount.toString()).toBe("2");

	// check and expect next authorities count to be 3
	let nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();
	// @ts-ignore
	expect(nextAuthorities.length.toString()).toBe("3");

	// force new era for staking elections
	let forceNewEra = polkadotApi.tx.staking.forceNewEraAlways();
	const forceNewEraAlwayCall = polkadotApi.tx.sudo.sudo({
		callIndex: forceNewEra.callIndex,
		args: forceNewEra.args,
	});
	await forceNewEraAlwayCall.signAndSend(alice);

	// chill(remove) charlie as validator
	let call = polkadotApi.tx.staking.chill();
	await call.signAndSend(charlieStash);
	await waitForEvent(polkadotApi, 'staking', 'Chilled');

	// wait for the next session
	await waitForTheNextSession(polkadotApi);

	// check and expect threshold count to be 1
	thresholdCount = await polkadotApi.query.dkg.signatureThreshold();
	expect(thresholdCount.toString()).toBe("1");

	// check and expect next authorities count to be 2
	nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();
	// @ts-ignore
	expect(nextAuthorities.length.toString()).toBe("2");
}
