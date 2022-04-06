 # DKG Proposal Handler Module

 A pallet to handle unsigned and signed proposals that are submitted for signing by the DKG.

 ## Overview

 The DKG Proposal Handler pallet is the pallet that directly handles the unsigned and
 signed DKG proposals. It is responsible for maintaining the `UnsignedProposalQueue` that the
 DKG authorities poll from for initiating threshold-signing. It is also responsible for the
 submission of signed proposals back on-chain, which allows for external relayers to listen and
 relay the signed proposals to their destinations.

 The pallet is meant to be used in conjunction with any governance system that processes
 unsigned proposals either directly or indirectly such as the `pallet-dkg-proposals` pallet,
 which delegates successfully voted upon proposals to the DKG Proposal Handler for processing.
 This pallet also contains root-level functions that allow for the submission of unsigned
 proposals that are useful for Webb Protocol applications. The intention being that tokenholders
 of the Webb Protocol chain can vote through the `pallet-democracy` or a similar governance
 system to submit unsigned proposals relevant for protocols built on the Webb Protocol's
 interoperable private application platform.

 The signed proposals are submitted on-chain through an offchain worker and storage mechanism
 that is maintained locally by each DKG authority. The DKG authorities engage in an offchain
 multi-party ECDSA threshold signing protocol to sign the unsigned proposals. Once the DKG
 authorities have signed proposals, they submit the signed proposals on-chain, where the
 signatures are verified against the active DKG's public key.

 ### Terminology

 - Unsigned Proposal: A Proposal that is unsigned and is ready to be signed by the DKG
   authorities.
 - Signed Proposal: A Proposals that is signed and contains a signature from the active DKG in
   the respective round.
 - Unsigned Proposal Queue: A queue of unsigned proposals that are ready for signing.
 - Anchor Update Proposal: A proposal for updating the merkle root state of an anchor on some
   compatible blockchain.
 - Refresh Proposal: The proposal which rotates a soon-to-be outdated active DKG key to the
   soon-to-be active next DKG key.
 - Proposer Set Update Proposal: The proposal which updates the latest proposer set from
   `pallet-dkg-proposals`.

 ### Implementation

 The DKG Proposal Handler pallet is implemented with the primary purpose of handling unsigned
 proposals from the `pallet-dkg-proposals`, i.e. Anchor Update Proposals, handling forcefully
 submitting unsigned proposals from the integrating chain's tokenholders, and handling the
 submission of signed proposals back on-chain for data provenance and further processing.

 There are two main methods for submitting unsigned proposals currently implemented:
 1. `handle_unsigned_proposal` - A generic handler which expects raw Anchor Update Proposals.
 2. `force_submit_unsigned_proposal` - A root-level extrinsic that allows for the submission of
 all other valid unsigned proposals

 Handled unsigned proposals are added to the `UnsignedProposalQueue` and are processed by the DKG
 authorities offchain. The queue is polled using a runtime API and the multi-party ECDSA
 threshold signing protocol is initiated for each proposal. Once the DKG authorities have signed
 the unsigned proposal, the proposal is submitted on-chain and an event is emitted.
 Signed proposals are stored in the offchain storage system and polled each block by the offchain
 worker system.

 The types of proposals available for submission is defined in the `ProposalType` enum as well as
 the `DKGPayloadKey` enum. The list of currently supported proposals is as follows:
 - Refresh: A proposal to refresh the DKG key across authority changes.
 - ProposerSetUpdate: A proposal to update the proposer set from `pallet-dkg-proposals`.
 - EVM: A generic EVM transaction proposal.
 - AnchorCreate: A proposal to create an anchor on a compatible blockchain.
 - AnchorUpdate: A proposal to update an anchor state on a compatible blockchain.
 - TokenAdd: A proposal to add a token to system supporting a many-to-one token wrapper.
 - TokenRemove: A proposal to remove a token from system supporting a many-to-one token wrapper.
 - WrappingFeeUpdate: A proposal to update the wrapping fee for a many-to-one token wrapper.
 - ResourceIdUpdate: A proposal to update or add a new resource ID to a system for registering
   resources.
 - RescueTokens: A proposal to rescue tokens from a treasury based system.
 - MaxDepositLimitUpdate: A proposal to update the maximum deposit limit for an escrow system.
 - MinWithdrawalLimitUpdate: A proposal to update the minimal withdrawal limit for an escrow
   system.
 - SetVerifier: A proposal to update the verifier for a zkSNARK based system.
 - SetTreasuryHandler: A proposal to update the treasury handler for a treasury based system.
 - FeeRecipientUpdate: A proposal to update the fee recipient for an escrow system.

 ### Rewards

 Currently, there are no extra rewards integrated for successfully signing proposals. This is a
 future feature.

 ## Related Modules

 * [`System`](https://github.com/paritytech/substrate/tree/master/frame/system)
 * [`Support`](https://github.com/paritytech/substrate/tree/master/frame/support)
 * [`DKG Proposals`](https://github.com/webb-tools/dkg-substrate/blob/664aebd10e6c1dc9e787a0465fd36b60e5e82c0d/pallets/dkg-proposals)
