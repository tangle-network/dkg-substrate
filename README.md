<h1 align="center">Webb DKG</h1>

<p align="center">
    <strong>🕸️  The Webb DKG  🧑‍✈️</strong>
    <br />
    <sub> ⚠️ Beta Software ⚠️ </sub>
</p>

<br />

## Running the DKG

Currently the easiest way to run the DKG is to use a 3-node local testnet using `dkg-standalone-node`. We will call those nodes `Alice`, `Bob` and
`Charlie`. Each node will use the built-in development account with the same name, i.e. node `Alice` will use the `Alice` development
account and so on. Each of the three accounts has been configured as an initial authority at genesis. So, we are using three validators
for our testnet.

`Alice` is our bootnode and is started like so:

```
$ RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --alice
```

`Bob` is started like so:

```
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --bob
```

`Charlie` is started like so:

```
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --charlie
```

Note that the examples above use an ephemeral DB due to the `--tmp` CLI option. If you want a persistent DB, use `--/tmp/[node-name]`
instead. Replace `node-name` with the actual node name (e.g. `alice`) in order to assure separate dirctories for the DB.
## Build & Run

Follow these steps to prepare a local Substrate development environment :hammer_and_wrench:

### Setup of Machine

If necessary, refer to the setup instructions at the
[Substrate Developer Hub](https://substrate.dev/docs/en/knowledgebase/getting-started/#manual-installation).

### Build

Once the development environment is set up, build the DKG. This command will
build the
[Wasm Runtime](https://substrate.dev/docs/en/knowledgebase/advanced/executor#wasm-execution) and
[native](https://substrate.dev/docs/en/knowledgebase/advanced/executor#native-execution) code:

```bash
cargo build --release
```

> NOTE: You _must_ use the release builds! The optimizations here are required
> as in debug mode, it is expected that nodes are not able to run fast enough to produce blocks.

### Troubleshooting for Apple Silicon users

Install Homebrew if you have not already. You can check if you have it installed with the following command:

```bash
brew help
```

If you do not have it installed open the Terminal application and execute the following commands:

```bash
# Install Homebrew if necessary https://brew.sh/
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"

# Make sure Homebrew is up-to-date, install openssl
brew update
brew install openssl
```

❗ **Note:** Native ARM Homebrew installations are only going to be supported at `/opt/homebrew`. After Homebrew installs, make sure to add `/opt/homebrew/bin` to your PATH.

```bash
echo 'export PATH=/opt/homebrew/bin:$PATH' >> ~/.bash_profile
```

In order to build **dkg-substrate** in `--release` mode using `aarch64-apple-darwin` Rust toolchain you need to set the following environment variables:

```bash
echo 'export RUSTFLAGS="-L /opt/homebrew/lib"' >> ~/.bash_profile
```

## Relay Chain

> **NOTE**: In the following two sections, we document how to manually start a few relay chain
> nodes, start a parachain node (collator), and register the parachain with the relay chain.
>
> We also have the [**`polkadot-launch`**](https://www.npmjs.com/package/polkadot-launch) CLI tool
> that automate the following steps and help you easily launch relay chains and parachains. However
> it is still good to go through the following procedures once to understand the mechanism for running
> and registering a parachain.

To operate a parathread or parachain, you _must_ connect to a relay chain. Typically you would test
on a local Rococo development network, then move to the testnet, and finally launch on the mainnet.
**Keep in mind you need to configure the specific relay chain you will connect to in your collator
`chain_spec.rs`**. In the following examples, we will use `rococo-local` as the relay network.

### Build Relay Chain

Clone and build [Polkadot](https://github.com/paritytech/polkadot) (be aware of the version tag we used):

```bash
# Get a fresh clone, or `cd` to where you have polkadot already:
git clone -b v0.9.7 --depth 1 https://github.com/paritytech/polkadot.git
cd polkadot
cargo build --release
```

### Generate the Relay Chain Chainspec

First, we create the chain specification file (chainspec). Note the chainspec file _must_ be generated on a
_single node_ and then shared among all nodes!

👉 Learn more about chain specification [here](https://substrate.dev/docs/en/knowledgebase/integrate/chain-spec).

```bash
./target/release/polkadot build-spec \
--chain rococo-local \
--raw \
--disable-default-bootnode \
> rococo_local.json
```

### Start Relay Chain

We need _n + 1_ full _validator_ nodes running on a relay chain to accept _n_ parachain / parathread
connections. Here we will start two relay chain nodes so we can have one parachain node connecting in
later.

From the Polkadot working directory:

```bash
# Start Relay `Alice` node
./target/release/polkadot \
--chain ./rococo_local.json \
-d /tmp/relay/alice \
--validator \
--alice \
--port 50555
```

Open a new terminal, same directory:

```bash
# Start Relay `Bob` node
./target/release/polkadot \
--chain ./rococo_local.json \
-d /tmp/relay/bob \
--validator \
--bob \
--port 50556
```

Add more nodes as needed, with non-conflicting ports, DB directories, and validator keys
(`--charlie`, `--dave`, etc.).

### Reserve a ParaID

To connect to a relay chain, you must first \_reserve a `ParaId` for your parathread that will
become a parachain. To do this, you will need sufficient amount of currency on the network account
to reserve the ID.

In this example, we will use **`Charlie` development account** where we have funds available.
Once you submit this extrinsic successfully, you can start your collators.

The easiest way to reserve your `ParaId` is via
[Polkadot Apps UI](https://polkadot.js.org/apps/?rpc=ws%3A%2F%2F127.0.0.1%3A9944#/parachains/parathreads)
under the `Parachains` -> `Parathreads` tab and use the `+ ParaID` button.

## Parachain

### Select the Correct Relay Chain

To operate your parachain, you need to specify the correct relay chain you will connect to in your
collator `chain_spec.rs`. Specifically you pass the command for the network you need in the
`Extensions` of your `ChainSpec::from_genesis()` [in the code](node/src/chain_spec.rs#78-81).

```rust
Extensions {
	relay_chain: "rococo-local".into(), // You MUST set this to the correct network!
	para_id: id.into(),
},
```

> You can choose from any pre-set runtime chainspec in the Polkadot repo, by referring to the
> `cli/src/command.rs` and `node/service/src/chain_spec.rs` files or generate your own and use
> that. See the [Cumulus Workshop](https://substrate.dev/cumulus-workshop/) for how.

In the following examples, we will use the `rococo-local` relay network we setup in the last section.

### Export the DKG Genesis and Runtime

We first generate the **genesis state** and **genesis wasm** needed for the parachain registration.

```bash
# Build the dkg node (from it's top level dir)
cd dkg-substrate
cargo build --release

# Folder to store resource files needed for parachain registration
mkdir -p resources

# Build the chainspec
./target/release/dkg-standalone-node build-spec \
--disable-default-bootnode > ./resources/template-local-plain.json

# Build the raw chainspec file
./target/release/dkg-standalone-node build-spec \
--chain=./resources/template-local-plain.json \
--raw --disable-default-bootnode > ./resources/template-local-raw.json

# Export genesis state to `./resources`, using 2000 as the ParaId
./target/release/dkg-standalone-node export-genesis-state --parachain-id 2000 > ./resources/para-2000-genesis

# Export the genesis wasm
./target/release/dkg-standalone-node export-genesis-wasm > ./resources/para-2000-wasm
```

> **NOTE**: we have set the `para_ID` to be **2000** here. This _must_ be unique for all parathreads/chains
> on the relay chain you register with. You _must_ reserve this first on the relay chain for the
> testnet or mainnet.

### Start a DKG Node (Collator)

From the dkg-substrate working directory:

```bash
# NOTE: this command assumes the chain spec is in a directory named `polkadot`
# that is at the same level of the template working directory. Change as needed.
#
# It also assumes a ParaId of 2000. Change as needed.
./target/release/dkg-standalone-node \
-d /tmp/parachain/alice \
--collator \
--alice \
--force-authoring \
--ws-port 9945 \
--parachain-id 2000 \
-- \
--execution wasm \
--chain ../polkadot/rococo_local.json
```

### Parachain Registration

Now that you have two relay chain nodes, and a parachain node accompanied with a relay chain light
client running, the next step is to register the parachain in the relay chain with the following
steps (for detail, refer to the [Substrate Cumulus Worship](https://substrate.dev/cumulus-workshop/#/en/3-parachains/2-register)):

-   Goto [Polkadot Apps UI](https://polkadot.js.org/apps/#/explorer), connecting to your relay chain.

-   Execute a sudo extrinsic on the relay chain by going to `Developer` -> `sudo` page.

-   Pick `paraSudoWrapper` -> `sudoScheduleParaInitialize(id, genesis)` as the extrinsic type,
    shown below.

        ![Polkadot Apps UI](docs/assets/ss01.png)

-   Set the `id: ParaId` to 2,000 (or whatever ParaId you used above), and set the `parachain: Bool`
    option to **Yes**.

-   For the `genesisHead`, drag the genesis state file exported above, `para-2000-genesis`, in.

-   For the `validationCode`, drag the genesis wasm file exported above, `para-2000-wasm`, in.

> **Note**: When registering to the public Rococo testnet, ensure you set a **unique** `paraId`
> larger than 1,000. Values below 1,000 are reserved _exclusively_ for system parachains.

### Restart the DKG (Collator)

The DKG node may need to be restarted to get it functioning as expected. After a
[new epoch](https://wiki.polkadot.network/docs/en/glossary#epoch) starts on the relay chain,
your parachain will come online. Once this happens, you should see the collator start
reporting _parachain_ blocks:

**Note the delay here!** It may take some time for your relay chain to enter a new epoch.

### Code Coverage

You need to have docker installed to generate code coverage.

```sh
docker build -t cov -f docker/Coverage.Dockerfile .
```

```sh
docker run --security-opt seccomp=unconfined cov
```
### Pallets

The DKG runtime is uses the following pallets which are central to how the protocol functions.

## pallet-dkg-metadata

This pallet essentially tracks the information about the current and next authority sets, including the set Ids.
It does this by implementing the `OneSessionHandler` trait which allows it to receieve both new and queued authority sets when the session changes.

## pallet-dkg-proposals

This pallet handles maintaining valid proposers and also voting on proposals.
The valid set of proposers is equivalent to the current DKG authorities. This pallet gains access to the current DKG authorities by
implementing the `OnAuthoritySetChangedHandler` trait, that way it is able to receive the updated DKG authorities from `pallet-dkg-metadata`.

This pallet maintains a queue for pending proposals which the DKG authorities vote on and if the vote threshold is met, the proposal is passed on to be handled
by a type that implements the `ProposalHandlerTrait`.

In this current iteration the proposals are Ethereum transactions.

## pallet-dkg-proposal-handler

This pallet implements the `ProposalHandlerTrait` accepts proposals and signs them using the DKG authority keys.

## pallet-dkg-mmr

This pallet serves as a leaf provider for the `pallet-mmr`, generating leaf data that contains a merke root hash for a particular authority set.

It also provides a type that has an implementation for converting `ECDSA` keys to ethereum compatible keys.


### Note on Offchain workers

The DKG makes use of offchain workers to submit some extrinsics back on chain and the runtime validates that the origin of such extrinsics is part of the active or queued authoritiy set, if running a development node or a local test net, the sr25519 account keys
for the predefined validators Alice, Bob, etc, have been added to the keystore for convenience.

If running a live chain as a validator or collator, please add your sr25519 account keys to the node's local keystore either by using the `author_insertKey` RPC or using the `key` subcommand (`dkg-standalone-node key insert --key-type acco --scheme sr25519 --suri <path-secret-phrase>`) of the node cli

> Key Type is acco
> Scheme is sr25519

**Note**
For the standalone node the account being added to the keystore should be the Stash account used in staking not the Controller account
