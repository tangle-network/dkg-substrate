# dkg-gossip
The purpose of the DKG gossip package is to provide a gossip interface for sending and receiving messages between nodes in a distributed key generation protocol. This protocol *may* also include sending messages that are not part of the DKG protocol, but are used to communicate between nodes in the DKG protocol to agree on various parameters and state necessary for the DKG protocol to work.

## TODO
We will aim to clean out the existing `transactions.rs` file from its transaction-specific logic. We will replace it with *messages* and a *message queue* instead of a transaction pool. The motivation is that we will consume messages from the queue, process them in the `dkg-meta-handler`, and then gather all outgoing messages and send them over the wire using this `dkg-gossip` package.

### Key questions
1. How to hook this up to the `dkg-meta-handler` and connect them?
2. How to populate the message queue and do we need to? Maybe we can just supply the meta handler directly in some form.