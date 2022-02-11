import {TokenAddProposal, TokenRemoveProposal, WrappingFeeUpdateProposal} from "./utils";
import {resourceId} from "./resource";

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;
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
