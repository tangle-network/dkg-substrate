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

// A test suite for testing Keygen changes in the DKG System
// and to observe the behavior of the system.
//
// When we start, we expect the Keygen to equal to 2, then we wait for the first keygen to finish,
// then we update the Keygen to 3, expecting the system to now work with 3 Authorities.
// We then wait for the second keygen to finish, and then we update the Keygen back to 2,
// expecting the system to now work with 2 Authorities.

import fs from 'fs';
import { expect } from 'chai';
import { ChildProcess, execSync } from 'child_process';
import {
	startStandaloneNode,
	sudoTx,
	waitForTheNextSession,
	fetchDkgPublicKey,
	sleep,
	waitForTheNextDkgPublicKey,
	endpoint,
} from '../utils/setup';
import { ApiPromise, WsProvider } from '@polkadot/api';
import { Vec } from '@polkadot/types';
import { BLOCK_TIME } from '../utils/constants';

describe('Keygen Changes Flow', function () {
	// 8 sessions should be more than enough for the test to complete
	this.timeout(80 * BLOCK_TIME);
	// 4 session.
	this.slow(40 * BLOCK_TIME);
	// fail fast, since tests are dependent on each other.
	this.bail(true);

	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;
	let api: ApiPromise;

	before(async () => {
		// delete the tmp directory if it exists.
		const gitRoot = execSync('git rev-parse --show-toplevel').toString().trim();
		const tmpDir = `${gitRoot}/tmp`;
		if (fs.existsSync(tmpDir)) {
			fs.rmSync(tmpDir, { recursive: true });
		}
		aliceNode = startStandaloneNode('alice', { tmp: true, printLogs: false });
		bobNode = startStandaloneNode('bob', { tmp: true, printLogs: false });
		charlieNode = startStandaloneNode('charlie', {
			tmp: true,
			printLogs: false,
		});

		api = await ApiPromise.create({
			provider: new WsProvider(endpoint),
		});
	});

	// This test requires at least 3 sessions.
	// 3 sessions = 3 * 10 blocks = 30 blocks = 30 * 6 seconds = 180 seconds.
	it('should be able to increase the Keygen Threshold', async () => {
		// first query the current keygen threshold value.
		const currentKeygenThreshold = await api.query.dkg.keygenThreshold();
		expect(currentKeygenThreshold.toHex()).to.equal(
			'0x0002',
			'Keygen threshold at the start should be 2'
		);
		// then, we shall query the current best authorities.
		const currentBestAuthorities = await api.query.dkg.bestAuthorities();
		expect(currentBestAuthorities.length).to.equal(
			2,
			'Current best authorities should be 2'
		);
		// and we wait for the first keygen to be completed.
		console.log('before wait for next dkg public key');
		await waitForTheNextDkgPublicKey(api);
		console.log('after wait for next dkg public key');
		// next, is to increase the keygen threshold.
		const increaseKeygenThreshold = api.tx.dkg.setKeygenThreshold(3);
		await sudoTx(api, increaseKeygenThreshold);
		// and then, we query for the pending keygen threshold.
		// we expect the pending keygen threshold to be 3.
		const pendingKeygenThreshold = await api.query.dkg.pendingKeygenThreshold();
		expect(pendingKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Pending keygen threshold should be 3'
		);
		// now we wait for the next session, we expect the next keygen threshold to be 3.
		// we also expect the next best authorities to be 3.
		console.log('before the next session');
		await waitForTheNextSession(api);
		console.log('after the next session');
		await sleep(BLOCK_TIME * 2);
		const nextKeygenThreshold = await api.query.dkg.nextKeygenThreshold();
		expect(nextKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Next keygen threshold should be 3'
		);
		const nextBestAuthorities = await api.query.dkg.nextBestAuthorities();
		expect(nextBestAuthorities.length).to.equal(
			3,
			'Next best authorities should be 3'
		);

		// now, wait for the next session, we expect the keygen threshold to be 3.
		// and we expect the best authorities to be 3.
		// Also, the DKG rotation should still working as expected.
		console.log('before the next session');
		await waitForTheNextSession(api);
		console.log('after the next session');
		await sleep(BLOCK_TIME * 2);
		const keygenThreshold = await api.query.dkg.keygenThreshold();
		expect(keygenThreshold.toHex()).to.equal(
			'0x0003',
			'Keygen threshold should be now equal to 3'
		);
		const bestAuthorities = await api.query.dkg.bestAuthorities();
		expect(bestAuthorities.length).to.equal(3, 'Best authorities should be 3');
		// query the current DKG public key.
		const currentDkgPublicKey = await fetchDkgPublicKey(api);
		// now wait for the next session, we expect the dkg to rotate.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		// query the next DKG public key.
		const newDkgPublicKey = await fetchDkgPublicKey(api);
		// we expect the new dkg public key to be different from the current dkg public key.
		expect(newDkgPublicKey).to.not.equal(
			currentDkgPublicKey,
			'DKG public key should be different after a rotation'
		);
	});

	// This test requires at least 3 sessions.
	// 3 sessions = 3 * 10 blocks = 30 blocks = 30 * 6 seconds = 180 seconds.
	// **NOTE**: this test is also dependent on the previous test.
	it('should be able to decrease the Keygen Threshold', async () => {
		// first query the current keygen threshold value.
		const currentKeygenThreshold = await api.query.dkg.keygenThreshold();
		expect(currentKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Keygen threshold at the start should be 3'
		);
		// then, we shall query the current best authorities.
		const currentBestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const currentBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			currentBestAuthoritiesValue.toU8a()
		);
		expect(currentBestAuthorities.length).to.equal(
			3,
			'Current best authorities should be 3'
		);
		// next, is to decrease the keygen threshold.
		const decreaseKeygenThreshold = api.tx.dkg.setKeygenThreshold(2);
		await sudoTx(api, decreaseKeygenThreshold);
		// and then, we query for the pending keygen threshold.
		// we expect the pending keygen threshold to be 2.
		const pendingKeygenThreshold = await api.query.dkg.pendingKeygenThreshold();
		expect(pendingKeygenThreshold.toHex()).to.equal(
			'0x0002',
			'Pending keygen threshold should be 2'
		);
		// now we wait for the next session, we expect the next keygen threshold to be 2.
		// we also expect the next best authorities to be 2.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const nextKeygenThreshold = await api.query.dkg.nextKeygenThreshold();
		expect(nextKeygenThreshold.toHex()).to.equal(
			'0x0002',
			'Next keygen threshold should be 2'
		);
		const nextBestAuthoritiesValue = await api.query.dkg.nextBestAuthorities();
		const nextBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			nextBestAuthoritiesValue.toU8a()
		);
		expect(nextBestAuthorities.length).to.equal(
			2,
			'Next best authorities should be 2'
		);

		// now, wait for the next session, we expect the keygen threshold to be 2.
		// and we expect the best authorities to be 2.
		// Also, the DKG rotation should still working as expected.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const keygenThreshold = await api.query.dkg.keygenThreshold();
		expect(keygenThreshold.toHex()).to.equal(
			'0x0002',
			'Keygen threshold should be now equal to 2'
		);
		const bestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const bestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			bestAuthoritiesValue.toU8a()
		);
		expect(bestAuthorities.length).to.equal(2, 'Best authorities should be 2');
		// query the current DKG public key.
		const currentDkgPublicKey = await fetchDkgPublicKey(api);
		// now wait for the next session, we expect the dkg to rotate.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		// query the next DKG public key.
		const newDkgPublicKey = await fetchDkgPublicKey(api);
		// we expect the new dkg public key to be different from the current dkg public key.
		expect(newDkgPublicKey).to.not.equal(
			currentDkgPublicKey,
			'DKG public key should be different after a rotation'
		);
	});

	after(async () => {
		await api?.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await sleep(BLOCK_TIME * 2);
	});
});
