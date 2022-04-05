 # DKG Proposals Module

 A pallet to manage proposals that are submitted for signing by the DKG.

 ## Overview

 The DKG proposals pallet manages a governance system derived from
 ChainSafe's ChainBridge Substrate pallet. It is designed as the first
 layer in Webb's DKG governance system and is responsible for managing
 proposal submission and voting for messages that are intended to be signed
 by the DKG threshold signing protocol.

 The pallet implements a simple threshold voting system wherein proposers
 propose messages to be signed. Once a threshold of votes over the same
 proposal is met, the message is handled by a generic proposal handler.
 This pallet is intended to be used in conjunction with [`pallet-dkg-proposal-handler`].

 ### Terminology

 - Proposer: A valid account that can submit and vote on proposals.
 - Proposal: A message that is submitted, voted on, and eventually handled or rejected.
 - ProposerSet: The merkle root of the smallest merkle tree containing the ordered proposers.

 ### Implementation

 The DKG proposal system combines a set of proposers, a generic proposal message,
 and a threshold-voting system to build a simple governance system for "handling"
 proposals. By "handling", we intend for successful proposals to be sent to
 a secondary system that acts upon proposal data.

 In the Webb Protocol, the handler submits successful proposals to the DKG for signing.

 The proposers of the pallet are derived from the active authorities of the underlying
 chain as well as any account added to the set using the `add_proposer` call. The
 intention is for the set of proposers to grow larger than simply the authority set
 of the chain without growing the signing set of the underlying DKG.

 Proposers are required to submit 2 types of keys: AccountId keys and ECDSA keys. The former
 keys are used to propose and interact with the Substrate based chain integrating this pallet.
 The latter are used to interoperate with EVM systems who utilize the proposers for auxiliary
 protocols described below. This aligns non-authority proposers with authority proposers as well
 since we expect authorities to have both types of keys registered for consensus and DKG
 activities.

 The proposals of the system are generic and left to be handled by a proposal handler.
 Currently, upon inspection of the `pallet-dkg-proposal-handler` module, the only valid
 proposal that can be proposed and handled successfully is the Anchor Update proposal:
 the proposal responsible for bridging different anchors together in the Webb Protocol.

 The system can be seen as a 2-stage oracle-like system wherein proposers vote on
 events/messages they believe to be valid and, if successful, the DKG will sign such events.

 The system also supports accumulating proposers for off-chain auxiliary protocols that utilize
 proposers for new activities. In the Webb Protocol, we use the proposers to backstop the system
 against critical failures and provide an emergency fallback mechanism when the DKG fails to sign
 messages. We create a merkle tree of active proposers are submit the merkle root and session
 length to the DKG for signing so as to maintain the list of active proposers across the
 protocol's execution. If at any point in the future the DKG fails to sign messages and stops
 working, we can utilize this merkle root to allow proposers to vote to restart and transition
 any external system relying on the DKG to a new state.

 ### Rewards

 Currently, there are no extra rewards integrated for proposers. This is a future feature.

 ## Related Modules

 * [`System`](https://github.com/paritytech/substrate/tree/master/frame/system)
 * [`Support`](https://github.com/paritytech/substrate/tree/master/frame/support)
 * [`DKG Proposal Handler`](../../pallet-dkg-proposal-handler)