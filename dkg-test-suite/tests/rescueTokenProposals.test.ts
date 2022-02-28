import 'jest-extended';
import {
	RescueTokensProposal,
	ChainIdType,
	encodeFunctionSignature,
	encodeRescueTokensProposal,
	registerResourceId,
	waitForEvent,
} from '../src/utils';
import {ethers} from 'ethers';
import {TreasuryHandler, Treasury} from '@webb-tools/tokens';
import {Keyring} from '@polkadot/api';
import {u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {signAndSendUtil} from '../src/evm/util/utils';
import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	executeAfter, localChain2, wallet2
} from './utils/util';
import {SignatureBridgeSide} from "@webb-tools/bridges";
import {DeployerConfig} from "@webb-tools/interfaces";

describe('Rescue Token Proposal', () => {
	test('should be able to sign rescue token proposal', async () => {
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		const governedTokenAddress = anchor.token!;
		let treasuryHandlerToken = TreasuryHandler.connect(governedTokenAddress, wallet1);
		let initialGovernors = {
			[localChain.chainId]: wallet1,
			[localChain2.chainId]: wallet2,
		};
		const deployers: DeployerConfig = {
			[localChain.chainId]: wallet1,
			[localChain2.chainId]: wallet2,
		};
		const initialGovernor = initialGovernors[localChain.chainId];
		let bridgeInstance;
		// Create the bridgeSide
		bridgeInstance = await SignatureBridgeSide.createBridgeSide(
			initialGovernor,
			deployers[localChain.chainId],
		);
		const createTreasuryHandler = await TreasuryHandler.createTreasuryHandler(bridgeInstance.contract.address, [],[], bridgeInstance.admin)
		const createTreasury = await Treasury.createTreasury(createTreasuryHandler, wallet1);
		const resourceId = await Treasury.createResourceId(createTreasuryHandler, );
		// Create Mintable Token to add to GovernedTokenWrapper
		//Create an ERC20 Token
		const proposalPayload: RescueTokensProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					'rescueTokens(address,address,uint256,uint256)'
				),
				nonce: Number(await treasuryHandlerToken.contract.proposalNonce()) + 1,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			tokenAddress: '',
			toAddress: '',
			amount: '',
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeRescueTokensProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({type: 'sr25519'});
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'RescueTokens');
		const tokenAddProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop
			}
		});
		const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(tokenAddProposal);

		await signAndSendUtil(polkadotApi, proposalCall, alice);

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
				kind: 'RescueTokens';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).toEqual(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
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
		// Want to check that token was actually added
		//expect((await treasuryHandlerToken.contract.getTokens()).includes(tokenToAdd.contract.address)).toBeTrue();
	});

	afterAll(async () => {
		await executeAfter();
	});
});
