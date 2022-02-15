import {
	SubstrateProposal,
} from "../utils";
import {resourceId} from "./resource";

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;

export const wrappingFeeUpdateProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	encodedCall: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};
