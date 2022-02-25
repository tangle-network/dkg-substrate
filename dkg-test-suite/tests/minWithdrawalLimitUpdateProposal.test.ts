import 'jest-extended';
import {
	VAnchorConfigurableLimitProposal,
	ChainIdType,
	encodeFunctionSignature,
	encodeVAnchorConfigurableLimitProposal,
	registerResourceId,
	waitForEvent,
} from '../src/utils';
import {ethers} from 'ethers';
import {Anchors} from '@webb-tools/protocol-solidity';
import {Keyring} from '@polkadot/api';
import {u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';

import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	wallet2,
	localChain2, executeAfter
} from './utils/util';
import {signAndSendUtil} from "../src/evm/util/utils";

describe('Min Withdrawal Limit Update Proposal', () => {
	test('should be able to sign Min Withdrawal Limit Update Proposal', async () => {
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

		// create a deposit on localchain1
		const deposit = await anchor.deposit(localChain2.chainId);
		// now check the new merkel root.
		const newMerkleRoot1 = await anchor.contract.getLastRoot();
		expect(newMerkleRoot1).not.toEqual(merkleRoot1);
		const lastLeafIndex = deposit.index;
		// create anchor update proposal to be sent to the dkg.
		const resourceId = await anchor2.createResourceId();
		const proposalPayload: VAnchorConfigurableLimitProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					'configureLimits(uint256,uint256)'
				),
				nonce: 1,
				chainId: localChain2.chainId,
				chainIdType: ChainIdType.EVM,
			},
			min_withdrawal_limit_bytes: "0x1"
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeVAnchorConfigurableLimitProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({type: 'sr25519'});
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain2.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'MinWithdrawalLimitUpdate');
		const runtimeProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop
			}
		});
		const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(runtimeProposal);
		await signAndSendUtil(polkadotApi, proposalCall, alice);
		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
		// now we need to query the proposal and its signature.
		const key = {
			MinWithdrawlimitUpdateProposal: proposalPayload.header.nonce,
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
		const bridgeSide = signatureBridge.getBridgeSide(localChain2.chainId)!;

		const contract = bridgeSide.contract;
		// now we log the proposal data, signature, and if it is signed by the current governor or not.
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		const tx2 = await contract.executeProposalWithSignature(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		await expect(tx2.wait()).toResolve();
	});

	afterAll(async () => {
		await executeAfter();
	});
});
