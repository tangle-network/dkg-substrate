import { jest } from '@jest/globals';
import 'jest-extended';
import {
    AnchorUpdateProposal,
    ChainIdType,
	ethAddressFromUncompressedPublicKey,
	makeResourceId,
	startStandaloneNode,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { Anchors, Bridges } from '@webb-tools/protocol-solidity';
import { MintableToken } from '@webb-tools/tokens';
import { ApiPromise, WsProvider } from '@polkadot/api';
import { ACC1_PK, ACC2_PK, BLOCK_TIME } from '../src/constants';

describe('Anchor Update Proposal', () => {
	jest.setTimeout(100 * BLOCK_TIME); // 100 blocks

	let polkadotApi: ApiPromise;
	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;

	let localChain: LocalChain;
	let localChain2: LocalChain;
	let wallet1: ethers.Wallet;
	let wallet2: ethers.Wallet;

	let signatureBridge: Bridges.SignatureBridge;

	beforeAll(async () => {
		aliceNode = startStandaloneNode('alice', { tmp: true, printLogs: false });
		bobNode = startStandaloneNode('bob', { tmp: true, printLogs: false });
		charlieNode = startStandaloneNode('charlie', { tmp: true, printLogs: false });

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
			provider: new WsProvider('ws://127.0.0.1:9944'),
		});

		// Update the signature bridge governor.
		const dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
		expect(dkgPublicKey).toBeString();
		const governorAddress = ethAddressFromUncompressedPublicKey(dkgPublicKey);

		let intialGovernors = {
			[localChain.chainId]: wallet1,
			[localChain2.chainId]: wallet2,
		};

		// Depoly the signature bridge.
		signatureBridge = await localChain.deploySignatureBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			intialGovernors
		);
		const signatureSide = signatureBridge.getBridgeSide(localChain.chainId);
		const contract = signatureSide.contract;
		contract.connect(localChain.provider());
		// now we transferOwnership, forcefully.
		const tx = await contract.transferOwnership(governorAddress, 1);
		expect(tx.wait()).toResolve();
		// check that the new governor is the same as the one we just set.
		const currentGovernor = await contract.governor();
		expect(currentGovernor).toEqualCaseInsensitive(governorAddress);

		// get the anhor on localchain1
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		await anchor.setSigner(wallet1);
		// approve token spending
		const tokenAddress = signatureBridge.getWebbTokenAddress(localChain.chainId)!;
		const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
		await token.approveSpending(anchor.contract.address);
		await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));

		// do the same but on localchain2
		const anchor2 = signatureBridge.getAnchor(localChain2.chainId, ethers.utils.parseEther('1'))!;
		await anchor2.setSigner(wallet2);
		const tokenAddress2 = signatureBridge.getWebbTokenAddress(localChain2.chainId)!;
		const token2 = await MintableToken.tokenFromAddress(tokenAddress2, wallet2);
		await token2.approveSpending(anchor2.contract.address);
		await token2.mintTokens(wallet2.address, ethers.utils.parseEther('1000'));
	});

	test('should be able to sign Update Anchor Proposal', async () => {
		// get the anhor on localchain1
		const anchor = signatureBridge.getAnchor(
			localChain.chainId,
			ethers.utils.parseEther('1')
		)! as Anchors.Anchor;
		await anchor.setSigner(wallet1);
		// check the merkle root
		const merkleRoot1 = await anchor.contract.getLastRoot();
		// get the anchor on localchain2
		const anchor2 = signatureBridge.getAnchor(
			localChain2.chainId,
			ethers.utils.parseEther('1')
		)! as Anchors.Anchor;
		await anchor2.setSigner(wallet2);
		// check the merkle root
		const merkleRoot2 = await anchor2.contract.getLastRoot();

		// create a deposit on localchain1
		const deposit = await anchor.deposit(localChain2.chainId);
		// now check the new merkel root.
		const newMerkleRoot1 = await anchor.contract.getLastRoot();
		expect(newMerkleRoot1).not.toEqual(merkleRoot1);
		// create anchor update proposal to be sent to the dkg.
		const anchorHandlerAddress = await anchor2.getHandler();
		const resourceId = makeResourceId(anchorHandlerAddress, ChainIdType.EVM, localChain.chainId);
		const proposalPayload: AnchorUpdateProposal = {
			header: {
				resourceId,
				functionSignature: ''
				nonce: 0,
			}
		}
	});

	afterAll(async () => {
		await polkadotApi.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await localChain?.stop();
	});
});
