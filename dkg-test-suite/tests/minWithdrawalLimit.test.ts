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
    const vAnchor = signatureVBridge.getVAnchor(localChain.chainId)!;
    const resourceId = await vAnchor.createResourceId();
    // Create Mintable Token to add to GovernedTokenWrapper
    //Create an ERC20 Token
    const proposalPayload: MinWithdrawalLimitProposal = {
      header: {
        resourceId,
        functionSignature: encodeFunctionSignature(
          vAnchor.contract.interface.functions['configureMinimalWithdrawalLimit(uint256)'].format()
        ),
        nonce: Number(await vAnchor.contract.getProposalNonce()) + 1,
        chainId: localChain.chainId,
        chainIdType: ChainIdType.EVM
      },
      minWithdrawalLimitBytes: "0x50",
    };
    // register proposal resourceId.
    await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
    const proposalBytes = encodeMinWithdrawalLimitProposal(proposalPayload);
    // get alice account to send the transaction to the dkg node.
    const keyring = new Keyring({type: 'sr25519'});
    const alice = keyring.addFromUri('//Alice');
    const prop = u8aToHex(proposalBytes);
    const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
      Evm: localChain.chainId,
    });
    const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'MinWithdrawalLimitUpdate');
    const minWithdrawalLimitProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
      Unsigned: {
        kind: kind,
        data: prop
      }
    });
    const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(minWithdrawalLimitProposal);

    await signAndSendUtil(polkadotApi, proposalCall, alice);

    // now we need to wait until the proposal to be signed on chain.
    await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
    // now we need to query the proposal and its signature.
    const key = {
      MinWithdrawalLimitUpdateProposal: proposalPayload.header.nonce,
    };
    const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
    const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
    expect(value.isSome).toBeTrue();
    const dkgProposal = value.unwrap().toJSON() as {
      signed: {
        kind: 'MinWithdrawalLimitUpdate';
        data: HexString;
        signature: HexString;
      };
    };
    // sanity check.
    expect(dkgProposal.signed.data).toEqual(prop);
    // perfect! now we need to send it to the signature bridge.
    const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.chainId);
    const contract = bridgeSide.contract;
    const isSignedByGovernor = await contract.isSignatureFromGovernor(
      dkgProposal.signed.data,
      dkgProposal.signed.signature
    );
    expect(isSignedByGovernor).toBeTrue();
    // check that we have the resouceId mapping.
    const tx2 = await contract.executeProposalWithSignature(
      dkgProposal.signed.data,
      dkgProposal.signed.signature
    );
    await expect(tx2.wait()).toResolve();
    // Want to check that fee was updated
    const minWithdrawalLimit = await vAnchor.contract.minimalWithdrawalAmount();
    expect(hexToNumber("0x50").toString()).toEqual(minWithdrawalLimit.toString());
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
