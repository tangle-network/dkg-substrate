const os = require('os');
const fs = require('fs');
// create a tmp dir for parachains data.

const tmpDir = os.tmpdir();
const mkTmpDir = (name) => `${tmpDir}/dkg/${name}`;
// delete the tmp dir if it exists.
if (fs.existsSync(mkTmpDir(''))){
  fs.rmSync(mkTmpDir(''), { recursive: true });
}
const chain = 'dev';

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

module.exports = {
  relaychain: {
    bin: "../../../paritytech/polkadot/target/release/polkadot",
    chain: "rococo-local",
    nodes: [
      {
        name: "alice",
        wsPort: 9944,
        port: 30444,
      },
      {
        name: "bob",
        wsPort: 9955,
        port: 30555,
      },
    ],
  },
  parachains: [
    {
      bin: "../target/release/dkg-collator",
      id: "2000",
      balance: "1000000000000000000000",
      chain,
      nodes: [
        {
          wsPort: 9988,
          name: "alice",
          port: 31200,
          basePath: mkTmpDir('alice'),
          flags,
        },
        {
          wsPort: 9989,
          name: "bob",
          port: 31201,
          basePath: mkTmpDir('bob'),
          flags,
        },
        {
          wsPort: 9990,
          name: "charlie",
          port: 31202,
          basePath: mkTmpDir('charlie'),
          flags,
        },
      ],
    },
  ],
  types: {},
};
