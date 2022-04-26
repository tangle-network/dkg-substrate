# MetaAsyncProtocolHandler documentation

The purpose of the ``MetaAsyncProtocolHandler`` is to easily and conveniently
spawn and execute keygen, offline, and voting stages **asynchronously** and in **parallel**. Prior to this addition, we used a ``MultiPartyECDSARounds``
structure to poll **synchronously** and **serially**. This implies each dkg node, after this addition, has higher throughput than the previous method of ``MultiPartyECDSARounds``.

In order to run each task in parallel, various fields inside the ``DKGWorker`` require ``Arc<RwLock<T>>`` or ``Arc<Mutex<T>>``. These fields are necessary for the
meta handler, since it must access various fields inside the DKGWorker from a parallel thread.

Inside src/worker.rs, there is a function ``spawn_async_protocol``. This code is called in place of ``MultiPartyECDSARounds::builder()``.
Inside ``spawn_async_protocol``, the function ``generate_async_proto_params`` is called. Here, the ``rounds`` or ``next_rounds`` field is set, leaving in its place a ``MetaAsyncProtocolRemote``. This remote
allows control over the meta handler, **including the submission of unsigned proposals**, checking the status, and, turning it off.

When a meta handler spawns, it immediately begins the keygen stage. After finishing keygen, the meta handler awaits patiently until it receives unsigned proposals. For each batch of unsigned proposals,
the meta handler automatically spawns multiple offline stages **concurrently**. Immediately after the conclusion of each individual offline stage, a consequent voting stage proceeds, allowing for concurrent offline->voting stages.
Once all the voting stages are complete, the meta handler will keep patiently awaiting for the next batch of unsigned proposals, unless, the respective meta remote calls ``shutdown``.

Inside src/worker.rs, the ``process_incoming_dkg_message`` function now properly broadcasts messages to the appropriate meta handlers. Each meta handler will automatically
discard any messages that are not meant for it, while processing those messages that are indeed meant for it.

### BlockChainIface
The ``BlockChainIface`` (short for BlockChainInterface) is used to interface the meta handlers with the blockchain. This is where the fields stored in the DKGWorker are shared.

### Testing
For most effective feedback for debugging, these commands should be used to inspect the DKGWorker and meta handlers
```
./target/release/dkg-standalone-node --tmp --alice
./target/release/dkg-standalone-node --tmp --bob
./target/release/dkg-standalone-node --tmp --charlie
```

### TODO
- Discuss the input parameters to the async protocols, and ensure is valid.
- Fix warnings of unused. There are still some unused parameters, such as the local key save path (needs to be implemented still in the meta handlers). Fixing the unused will ensure previous functionality remains the same
- When switching ``next_rounds`` to ``rounds``, call ``shutdown`` on the old ``rounds`` remote to free memory
- Refactoring
- Improve internal routing of messages to meta handlers (will speed up performance under heavy loads)
