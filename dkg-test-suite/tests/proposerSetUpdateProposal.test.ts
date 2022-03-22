
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
import {
	localChain,
	wallet1,
    polkadotApi,
	executeAfter,
	localChain2,
	wallet2,
    signatureBridge,
} from './utils/util';
import {jest} from "@jest/globals";
import {ApiPromise, Keyring} from "@polkadot/api";
import {ChildProcess} from "child_process";
import {Bridges} from "@webb-tools/protocol-solidity";
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import { u8aToBigInt, u8aToHex, u8aToString } from '@polkadot/util';
import { BigNumber, ethers } from 'ethers';
import { base58Encode, base58Decode } from '@polkadot/util-crypto/base58'
import ganache from 'ganache';

jest.setTimeout(10000 * BLOCK_TIME);


describe('Validator Node Test', () => {
	test('should be able to  remove validator node and check threshold', async () => {
        const provider = ganache.provider();
        const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
			None: 0,
		});
        const key = {
            ProposerSetUpdateProposal: 1,
        };
        const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
        const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
        expect(value.isSome).toBeTrue();
        const dkgProposal = value.unwrap().toJSON() as {
            signed: {
                kind: 'ProposerSetUpdate';
                data: HexString;
                signature: HexString;
            };
        };
        const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
        const contract = bridgeSide.contract;
        const isSignedByGovernor = await contract.isSignatureFromGovernor(
            dkgProposal.signed.data,
            dkgProposal.signed.signature
        );
        expect(isSignedByGovernor).toBeTrue();

        const proposalData = dkgProposal.signed.data.slice(2);
        const proposerSetRoot = `0x${proposalData.slice(0, 64)}`;
        const averageSessionLength = BigNumber.from(`0x${proposalData.slice(64, 80)}`);
        const numOfProposers = BigNumber.from(`0x${proposalData.slice(80, 88)}`);
        const proposalNonce = BigNumber.from(`0x${proposalData.slice(88, 96)}`);
        
        console.log(proposalData);
        console.log(proposerSetRoot);
        console.log(averageSessionLength);
        console.log(numOfProposers);
        console.log(proposalNonce);

        const tx = await contract.updateProposerSetData(
            proposerSetRoot,
            averageSessionLength,
            numOfProposers,
            proposalNonce,
            dkgProposal.signed.signature
        );
        await expect(tx.wait()).toResolve();

        // const contractProposerSetRoot = await bridgeSide.contract.proposerSetRoot();
		// expect(proposerSetRoot).toEqual(contractProposerSetRoot);

        // const contractAverageSessionLength = await bridgeSide.contract.averageSessionLengthInMillisecs();
		// expect(averageSessionLength).toEqual(BigNumber.from(contractAverageSessionLength));

        // const contractNumOfProposers = await bridgeSide.contract.numOfProposers();
		// expect(numOfProposers).toEqual(BigNumber.from(contractNumOfProposers));

        // const contractProposalNonce = await bridgeSide.contract.proposalNonce();
		// expect(proposalNonce).toEqual(BigNumber.from(contractProposalNonce));

        // // Now the proposer set root on the contract has been updated

        let proposerAccounts = await polkadotApi.query.dKGProposals.externalProposerAccounts.entries();
        let accounts = new Array();
        for (let i = 0; i<proposerAccounts.length; i++) {
            let account = proposerAccounts[i][1];
            accounts.push(account.toHuman());
        }

        console.log(accounts);

        let hash0 = ethers.utils.keccak256(accounts[0]);
        let hash1 = ethers.utils.keccak256(accounts[1]);
        let hash2 = ethers.utils.keccak256(accounts[2]);
        let hash3 = ethers.utils.keccak256('0x00');

        let hash01 = ethers.utils.keccak256(hash0.concat(hash1.slice(2)));
        let hash23 = ethers.utils.keccak256(hash2.concat(hash3.slice(2)));
        let root = ethers.utils.keccak256(hash01.concat(hash23.slice(2)));

        console.log('root', root);
    
        await provider.request({ method: "evm_setTime", params:[(await contract.lastGovernorUpdateTime()).add(600000).toHexString()]});

        await sleep(5000);
        
        const voteProposer0 = 
        {
          leafIndex: 0, 
          siblingPathNodes:[hash1, hash23], 
          proposedGovernor: '0x1111111111111111111111111111111111111111'
        };
    
        await contract.connect(accounts[0]).voteInFavorForceSetGovernor(voteProposer0);

        const voteProposer1 = 
        {
          leafIndex: 1, 
          siblingPathNodes:[hash0, hash23], 
          proposedGovernor: '0x1111111111111111111111111111111111111111'
        };

        await contract.connect(accounts[1]).voteInFavorForceSetGovernor(voteProposer1);

        const voteProposer2 = 
        {
        leafIndex: 2, 
        siblingPathNodes:[hash3, hash01], 
        proposedGovernor: '0x1111111111111111111111111111111111111111'
        };

        await contract.connect(accounts[2]).voteInFavorForceSetGovernor(voteProposer2);

        console.log(await contract.governor());
	});

    

	afterAll(async () => {
		await executeAfter();
	});
});
