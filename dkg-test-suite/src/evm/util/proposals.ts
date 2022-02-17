import {
	AnchorUpdateProposal,
	TokenAddProposal,
	TokenRemoveProposal,
	VAnchorConfigurableLimitProposal,
	WrappingFeeUpdateProposal,
	resourceId,
} from "./utils";

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;

export const anchorUpdateProposal: AnchorUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	srcChainId: 5001,
	lastLeafIndex: 0,
	merkleRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
};

export const tokenAddProposal: TokenAddProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newTokenAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const tokenRemoveProposal: TokenRemoveProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	removeTokenAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const wrappingFeeUpdateProposal: WrappingFeeUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newFee: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};


export const vAnchorConfigurableLimitProposal: VAnchorConfigurableLimitProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	min_withdrawal_limit_bytes: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};
