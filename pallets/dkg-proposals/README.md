# Pallet dkg-proposals

The DKG proposals is a governance pallet for voting on what proposals should be signed by the DKG. The pallet specifies a set of proposers who can propose proposals and has a whitelisting system for chains that we can make proposers for.

The voting process for deciding which proposals to sign is decided with simple threshold voting procedure. It is a multi-signature voting process. A certain amoint of proposers are required to vote before it can be handled by the `dkg-proposal-handler`. If there are 10 proposers we might require 6 of them to vote for the same proposal, for example.