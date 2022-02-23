import { jest } from '@jest/globals';
import 'jest-extended';
import {
	TokenAddProposal,
	ChainIdType,
	encodeFunctionSignature, encodeTokenAddProposal,
	encodeUpdateAnchorProposal,
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	registerResourceId,
	sleep,
	startStandaloneNode,
	waitForEvent,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { Anchors, Bridges } from '@webb-tools/protocol-solidity';
import { MintableToken } from '@webb-tools/tokens';
import { ApiPromise, Keyring, WsProvider } from '@polkadot/api';
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../src/constants';
import { u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';

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

		let initialGovernors = {
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
			initialGovernors
		);
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

		// now we set the resourceIds Mapping.
		const bridgeSide2 = signatureBridge.getBridgeSide(localChain2.chainId)!;
		await bridgeSide2.setResourceWithSignature(anchor2);
		// update the signature bridge governor on both chains.
		const sides = signatureBridge.bridgeSides.values();
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

	test('should be able to sign Token Add Proposal', async () => {
		// get the anchor on localchain1
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

		// create a deposit on localchain1
		const deposit = await anchor.deposit(localChain2.chainId);
		// now check the new merkel root.
		const newMerkleRoot1 = await anchor.contract.getLastRoot();
		expect(newMerkleRoot1).not.toEqual(merkleRoot1);
		const lastLeafIndex = deposit.index;
		// create anchor update proposal to be sent to the dkg.
		const resourceId = await anchor2.createResourceId();
		const proposalPayload: TokenAddProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					anchor.contract.interface.functions['updateEdge(uint256,bytes32,uint256)'].format()
				),
				nonce: 1,
				chainId: localChain2.chainId,
				chainIdType: ChainIdType.EVM,
			},
			newTokenAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeTokenAddProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({ type: 'sr25519' });
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain2.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenAdd');
		const runtimeProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop
			}
		});
		const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(runtimeProposal);
		const tx = new Promise<void>(async (resolve, reject) => {
			const unsub = await proposalCall.signAndSend(alice, ({ events, status }) => {
				if (status.isFinalized) {
					unsub();
					const success = events.find(({ event }) =>
						polkadotApi.events.system.ExtrinsicSuccess.is(event)
					);
					if (success) {
						resolve();
					} else {
						reject(new Error('Proposal failed'));
					}
				}
			});
		});
		await expect(tx).toResolve();
		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
		// now we need to query the proposal and its signature.
		const key = {
			TokenAddProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
		expect(value.isSome).toBeTrue();
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'TokenAdd';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).toEqual(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = signatureBridge.getBridgeSide(localChain2.chainId)!;
		// but first, we need to log few things to help us to debug.
		wallet2 = wallet2.connect(localChain2.provider());
		const contract = bridgeSide.contract.connect(wallet2);
		const currentGovernor = await contract.governor();
		const currentDkgPublicKey = await fetchDkgPublicKey(polkadotApi);
		const currentDkgAddress = ethAddressFromUncompressedPublicKey(currentDkgPublicKey!);
		expect(currentGovernor).toEqualCaseInsensitive(currentDkgAddress);
		// now we log the proposal data, signature, and if it is signed by the current governor or not.
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		// check that we have the resouceId mapping.
		const val = await contract._resourceIDToHandlerAddress(resourceId);
		const anchorHandlerAddress = await anchor2.getHandler();
		expect(val).toEqual(anchorHandlerAddress);
		const tx2 = await contract.executeProposalWithSignature(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		await expect(tx2.wait()).toResolve();
		// now we shall check the new merkle root on the other chain.
		const newMerkleRoots = await anchor2.contract.getLatestNeighborRoots();
		// the merkle root should be included now.
		expect(newMerkleRoots.includes(newMerkleRoot1)).toBeTrue();
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
