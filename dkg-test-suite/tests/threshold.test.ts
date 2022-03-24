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
import 'jest-extended';
import {BLOCK_TIME, SECONDS} from '../src/constants';
import {
  provider,
  sleep, startStandaloneNode, waitForEvent, waitForTheNextSession
} from '../src/utils';

import {jest} from "@jest/globals";
import {ApiPromise, Keyring} from "@polkadot/api";
import {ChildProcess} from "child_process";
import {Bridges} from "@webb-tools/protocol-solidity";

jest.setTimeout(10000 * BLOCK_TIME);

let polkadotApi: ApiPromise;
let aliceNode: ChildProcess;
let bobNode: ChildProcess;
let charlieNode: ChildProcess;

export let signatureBridge: Bridges.SignatureBridge;


async function executeAfter() {
  await polkadotApi.disconnect();
  aliceNode?.kill('SIGINT');
  bobNode?.kill('SIGINT');
  charlieNode?.kill('SIGINT');
  await sleep(20 * SECONDS);
}


describe('Validator Node Test', () => {
  test('should be able to  remove validator node and check threshold', async () => {
    aliceNode = startStandaloneNode('alice', {tmp: true, printLogs: false});
    bobNode = startStandaloneNode('bob', {tmp: true, printLogs: false});
    charlieNode = startStandaloneNode('charlie', {tmp: true, printLogs: false});

    polkadotApi = await ApiPromise.create({
      provider,
    });

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
  });

  afterAll(async () => {
    await executeAfter();
  });
});
