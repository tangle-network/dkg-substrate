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
import 'jest-extended';
import {jest} from "@jest/globals";
import {
	encodeFunctionSignature,
	registerResourceId,
	waitForEvent,
  startStandaloneNode,
  provider,
  waitUntilDKGPublicKeyStoredOnChain,
  ethAddressFromUncompressedPublicKey,
  sleep,
} from '../src/utils';
import {ethers} from 'ethers';
import {MintableToken, GovernedTokenWrapper} from '@webb-tools/tokens';
import {ApiPromise, Keyring} from '@polkadot/api';
import {hexToNumber, u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {MinWithdrawalLimitProposal, signAndSendUtil, ChainIdType, encodeMinWithdrawalLimitProposal} from '../src/evm/util/utils';
import { ChildProcess } from 'child_process';
import { LocalChain } from '../src/localEvm';
import { VBridge } from '@webb-tools/protocol-solidity';
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../src/constants';

describe('Wrapping Fee Update Proposal', () => {
  jest.setTimeout(100 * BLOCK_TIME); // 100 blocks

  let polkadotApi: ApiPromise;
  let aliceNode: ChildProcess;
  let bobNode: ChildProcess;
  let charlieNode: ChildProcess;
  let localChain: LocalChain;
  let localChain2: LocalChain;
  let wallet1: ethers.Wallet;
  let wallet2: ethers.Wallet;
  let signatureVBridge: VBridge.VBridge;

  beforeAll(async () => {
    aliceNode = startStandaloneNode('alice', {tmp: true, printLogs: false});
    bobNode = startStandaloneNode('bob', {tmp: true, printLogs: false});
    charlieNode = startStandaloneNode('charlie', {tmp: true, printLogs: false});

    localChain = new LocalChain('local', 5001, [
      {
        balance: ethers.utils.parseEther('1000').toHexString(),
        secretKey: ACC1_PK,
      },
      {
        balance: ethers.utils.parseEther('1000').toHexString(),
        secretKey: ACC2_PK,
      },
    ]);
    localChain2 = new LocalChain('local2', 5002, [
      {
        balance: ethers.utils.parseEther('1000').toHexString(),
        secretKey: ACC1_PK,
      },
      {
        balance: ethers.utils.parseEther('1000').toHexString(),
        secretKey: ACC2_PK,
      },
    ]);
    wallet1 = new ethers.Wallet(ACC1_PK, localChain.provider());
    wallet2 = new ethers.Wallet(ACC2_PK, localChain2.provider());
    // Deploy the token.
    const localToken = await localChain.deployToken('Webb Token', 'WEBB', wallet1);
    const localToken2 = await localChain2.deployToken('Webb Token', 'WEBB', wallet2);

    polkadotApi = await ApiPromise.create({
      provider,
    });

    // Update the signature bridge governor.
    const dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
    expect(dkgPublicKey).toBeString();
    const governorAddress = ethAddressFromUncompressedPublicKey(dkgPublicKey);

    let initialGovernors = {
      [localChain.chainId]: wallet1,
      [localChain2.chainId]: wallet2,
    };

    // Depoly the signature bridge.
    signatureVBridge = await localChain.deploySignatureVBridge(
      localChain2,
      localToken,
      localToken2,
      wallet1,
      wallet2,
      initialGovernors
    );
    // get the anchor on localchain1
    const vAnchor = signatureVBridge.getVAnchor(localChain.chainId)!;
    await vAnchor.setSigner(wallet1);
    // approve token spending
    const tokenAddress = signatureVBridge.getWebbTokenAddress(localChain.chainId)!;
    const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
    await token.approveSpending(vAnchor.contract.address);
    await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));

    // do the same but on localchain2
    const anchor2 = signatureVBridge.getVAnchor(localChain2.chainId)!;
    await anchor2.setSigner(wallet2);
    const tokenAddress2 = signatureVBridge.getWebbTokenAddress(localChain2.chainId)!;
    const token2 = await MintableToken.tokenFromAddress(tokenAddress2, wallet2);
    await token2.approveSpending(anchor2.contract.address);
    await token2.mintTokens(wallet2.address, ethers.utils.parseEther('1000'));

    // update the signature bridge governor on both chains.
    const sides = signatureVBridge.vBridgeSides.values();
    for (const signatureSide of sides) {
      const contract = signatureSide.contract;
      // now we transferOwnership, forcefully.
      const tx = await contract.transferOwnership(governorAddress, 1);
      expect(tx.wait()).toResolve();
      // check that the new governor is the same as the one we just set.
      const currentGovernor = await contract.governor();
      expect(currentGovernor).toEqualCaseInsensitive(governorAddress);
    }
  });
  test('should be able to update min withdrawal limit', async () => {

  });
  afterAll(async () => {
    await polkadotApi.disconnect();
    aliceNode?.kill('SIGINT');
    bobNode?.kill('SIGINT');
    charlieNode?.kill('SIGINT');
    await localChain?.stop();
    await localChain2?.stop();
    await sleep(5 * SECONDS);
	});
});
