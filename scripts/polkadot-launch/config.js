const os = require("os");
const fs = require("fs");
const { execSync } = require("child_process");
// create a tmp dir for parachains data.

const tmpDir = os.tmpdir();
const mkTmpDir = (name) => `${tmpDir}/dkg/${name}`;
// delete the tmp dir if it exists.
if (fs.existsSync(mkTmpDir(""))) {
  fs.rmSync(mkTmpDir(""), { recursive: true });
}

// a static counter we increment everytime we need a new node key.
let counter = 0;
const nextNodeKey = () => {
  counter += 1;
  return `00000000000000000000000000000000000000000000000000000000000000${counter}`;
};

const chain = "dev";
const flags = [
  "-ldkg=debug",
  "-ldkg_gadget=debug",
  "-lruntime::dkg_metadata=debug",
  "-lruntime::offchain=debug",
  "-lruntime::dkg_proposal_handler=debug",
  "-ldkg_proposals=debug",
  "-ldkg_gadget::async_protocol::keygen=debug",
  "-ldkg_gadget::async_protocol::keygen=debug",
  "-ldkg_gadget::gossip_engine::network=debug",
  "-ldkg_gadget::storage::public_keys=debug",
  "-ldkg_gadget::worker=debug",
  "--force-authoring",
  "--",
  "--execution=wasm",
];

// get the git root directory.
const gitRoot = execSync("git rev-parse --show-toplevel").toString().trim();

module.exports = {
  relaychain: {
    bin: "../../../../paritytech/polkadot/target/release/polkadot",
    chain: "rococo-local",
    nodes: [
      {
        name: "alice",
        wsPort: 9944,
        rpcPort: 9933,
        port: 30444,
      },
      {
        name: "bob",
        wsPort: 9955,
        rpcPort: 9934,
        port: 30555,
      },
    ],
  },
  parachains: [
    {
      bin: `${gitRoot}/target/release/dkg-collator`,
      id: "2000",
      balance: "1000000000000000000000",
      chain,
      nodes: [
        {
          name: "alice",
          wsPort: 9988,
          rpcPort: 9977,
          port: 31200,
          basePath: mkTmpDir("alice"),
          nodeKey: nextNodeKey(),
          flags,
        },
        {
          name: "bob",
          wsPort: 9989,
          rpcPort: 9978,
          port: 31201,
          basePath: mkTmpDir("bob"),
          nodeKey: nextNodeKey(),
          flags,
        },
        {
          name: "charlie",
          wsPort: 9990,
          rpcPort: 9979,
          port: 31202,
          basePath: mkTmpDir("charlie"),
          nodeKey: nextNodeKey(),
          flags,
        },
      ],
    },
  ],
  types: {},
};
