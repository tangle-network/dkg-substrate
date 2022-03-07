# Pallet dkg-proposals

A governance module for voting on proposals to be singed by the DKG.

# Overview

The DKG proposals is a governance pallet for voting on what proposals should be signed by the DKG. The pallet specifies a set of proposers who can propose proposals and has a whitelisting system for chains that we can make proposers for.

The voting process for deciding which proposals to sign is decided with simple threshold voting procedure. It is a multi-signature voting process. A certain amount of proposers are required to vote before it can be handled by the `dkg-proposal-handler`. If there are 10 proposers we might require 6 of them to vote for the same proposal, for example.

# Interface


# Dispatchable Functions
 - `set_maintainer` - called by the origin to set a maintainer
 - `force_set_maintainer` - called by the origin to forcefully set a maintainter.
 - `set_threshold` – called by the origin to set the threshold for proposals.
 - `set_resource` - called by the origin to set a method name on chain under an associated resource ID.
 - `remove-resource` - removes a resource with an associated resource ID
 - `whitelist_chain` - called to allow a chain be used as source or destination for bridge transfer
 - `add_proposer` - called by the origin to add a new proposer
 - `remove_proposer` - called to remove a proposer from the proposer set.
 - `acknowledge_proposal` - called to vote in favour of a provided proposal
 - `reject_proposal` - called to vote against a provided proposal
 - `eval_vote_state` - called to get the state of a proposal based on the threshold.