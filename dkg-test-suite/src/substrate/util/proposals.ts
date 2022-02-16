import {
	SubstrateProposal,
} from "../utils";
import {resourceId} from "./resource";

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;

export const wrappingFeeUpdateProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0x00000000',
		nonce,
	},
	call: 'Call::TokenWrapperHandler(Call::execute_wrapping_fee_proposal { r_id: [101, 120, 101, 99, 117, 116, 101, 95, 119, 114, 97, 112, 112, 105, 110, 103, 95, 102, 101, 101, 95, 112, 114, 111, 112, 111, 2, 0, 0, 0, 0, 5], wrapping_fee_percent: 5, into_pool_share_id: 1 })',
};

export const tokenAddProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0x00000000',
		nonce,
	},
	call: 'Call::TokenWrapperHandler(Call::execute_add_token_to_pool_share { r_id: [101, 120, 101, 99, 117, 116, 101, 95, 119, 114, 97, 112, 112, 105, 110, 103, 95, 102, 101, 101, 95, 112, 114, 111, 112, 111, 2, 0, 0, 0, 0, 5], name: [109, 101, 109, 101], asset_id: 1 })',
};

export const tokenRemoveProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0x00000000',
		nonce,
	},
	call: 'Call::TokenWrapperHandler(Call::execute_remove_token_from_pool_share { r_id: [101, 120, 101, 99, 117, 116, 101, 95, 114, 101, 109, 111, 118, 101, 95, 116, 111, 107, 101, 110, 95, 102, 114, 111, 109, 95, 2, 0, 0, 0, 0, 5], name: [109, 101, 109, 101], asset_id: 1 })',
};

export const anchorUpdateProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0x00000000',
		nonce,
	},
	call: 'Call::AnchorHandler(Call::execute_anchor_update_proposal { r_id: [101, 120, 101, 99, 117, 116, 101, 95, 97, 110, 99, 104, 111, 114, 95, 117, 112, 100, 97, 116, 101, 95, 112, 114, 111, 112, 2, 0, 0, 0, 0, 5], anchor_metadata: EdgeMetadata { src_chain_id: 2199023255553, root: Element([1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]), latest_leaf_index: 5 } })',
};

export const anchorCreateProposal: SubstrateProposal = {
	header: {
		resourceId,
		functionSignature: '0x00000000',
		nonce,
	},
	call: 'Call::AnchorHandler(Call::execute_anchor_create_proposal { deposit_size: 100, src_chain_id: 2199023255553, r_id: [101, 120, 101, 99, 117, 116, 101, 95, 97, 110, 99, 104, 111, 114, 95, 99, 114, 101, 97, 116, 101, 95, 112, 114, 111, 112, 2, 0, 0, 0, 0, 5], max_edges: 100, tree_depth: 32, asset: 0 })',
};