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
 */
import '@webb-tools/dkg-substrate-types';
import { waitForEvent, waitForTheNextSession } from './utils/setup';

import { Keyring } from '@polkadot/api';
import { expect } from 'chai';
import { polkadotApi } from './utils/util';

it('should be able to remove validator node and update thresholds', async () => {
	const keyring = new Keyring({ type: 'sr25519' });
	const charlieStash = keyring.addFromUri('//Charlie//stash');
	const alice = keyring.addFromUri('//Alice');

	// check and expect threshold count to be 2
	let sigthresholdCount = await polkadotApi.query.dkg.signatureThreshold();
	expect(sigthresholdCount.toString()).to.eq('2');

	// check and expect next authorities count to be 3
	let nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();
	expect(nextAuthorities.length.toString()).to.be.eq('3');

	// force new era for staking elections
	let forceNewEra = polkadotApi.tx.staking.forceNewEraAlways();
	await forceNewEra.signAndSend(alice);

	// chill(remove) charlie as validator
	let call = polkadotApi.tx.staking.chill();
	await call.signAndSend(charlieStash);
	await waitForEvent(polkadotApi, 'staking', 'Chilled');

	// wait for the next session
	await waitForTheNextSession(polkadotApi);

	// check and expect threshold count to be 1
	sigthresholdCount = await polkadotApi.query.dkg.signatureThreshold();
	expect(sigthresholdCount.toString()).to.eq('1');

	// check and expect threshold count to be 1
	let keygenthresholdCount = await polkadotApi.query.dkg.signatureThreshold();
	expect(keygenthresholdCount.toString()).to.eq('2');

	// check and expect next authorities count to be 2
	nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();
	expect(nextAuthorities.length.toString()).to.eq('2');
});
