const { ApiPromise, WsProvider } = require('@polkadot/api');
const { Keyring } = require('@polkadot/keyring');
const { hexToBytes, listenOneBlock } = require('./utils');
const ethers = require('ethers');
const { keccak256 } = require('@ethersproject/keccak256');
const { ECPair } = require('ecpair');

const ALICE = '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY';

const provider = new WsProvider('ws://127.0.0.1:9944');

const anchorUpdateProp = new Uint8Array([209, 1, 81, 230, 199, 165, 40, 225, 135, 229, 62, 97, 95, 108, 125, 232, 16, 159, 158, 30, 76, 163, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23, 194, 192, 194, 63, 104, 94, 171, 45, 188, 176, 167, 116, 213, 48, 159, 72, 180, 30, 153, 205, 53, 55, 38, 185, 131, 173, 100, 167, 96, 194, 18]);
const resourceId = hexToBytes("000000000000000003812879bc2cc702956671036463e6873f63178600000003");

async function testDirectProposal() {
    const api = await ApiPromise.create({
      provider,
      types: {
        ProposalType: {
          _enum: [
            'EVMUnsigned',
            'EVMSigned',
            'AnchorUpdate',
            'AnchorUpdateSigned',
            'TokenUpdate',
            'TokenUpdateSigned',
            'WrappingFeeUpdate',
            'WrappingFeeUpdateSigned'
          ]
        },
        EVMUnsigned: {
          data: 'Vec<u8>',
        },
        EVMSigned: {
          data: 'Vec<u8>',
          signature: 'Vec<u8>',
        },
        AnchorUpdate: {
          data: 'Vec<u8>',
        },
        AnchorUpdateSigned: {
          data: 'Vec<u8>',
          signature: 'Vec<u8>',
        },
        TokenUpdate: {
          data: 'Vec<u8>',
        },
        TokenUpdateSigned: {
          data: 'Vec<u8>',
          signature: 'Vec<u8>',
        },
        WrappingFeeUpdate: {
          data: 'Vec<u8>',
        },
        WrappingFeeUpdateSigned: {
          data: 'Vec<u8>',
          signature: 'Vec<u8>',
        }
      }
    });

    await sendSudoProposal(api);

    const dkgPubKeyCompressed = await api.query.dkg.dKGPublicKey();
    const dkgPubKey = ECPair.fromPublicKey(
        Buffer.from(dkgPubKeyCompressed[1].toHex().substr(2), 'hex'),
        { compressed: false },
    ).publicKey.toString('hex')

    const unsubSignedProps = await api.query.dKGProposalHandler.signedProposals(
        4,
        { anchorupdateproposal: 0 },
        (res) => {
            if (res) {
                const parsedResult = JSON.parse(JSON.stringify(res));
                console.log(`Signed prop: ${parsedResult}`);

                if (parsedResult) {
                    const sig = parsedResult.anchorUpdateSigned.signature;
                    console.log(`Signature: ${sig}`);

                    const propHash = keccak256(anchorUpdateProp)
                    const recoveredPubKey = ethers.utils.recoverPublicKey(propHash, sig).substr(2);
                    console.log(`Recovered public key: ${recoveredPubKey}`);
                    console.log(`DKG public key: ${dkgPubKey}`);

                    if (recoveredPubKey == dkgPubKey) {
                        console.log(`Public keys match`);
                    } else {
                        console.error(`Public keys do not match`);
                        process.exit();
                    }
                }
            }
        }
    );

    await new Promise(resolve => setTimeout(resolve, 20000));

    unsubSignedProps();
}

async function sendSudoProposal(api) {
    const keyring = new Keyring({ type: 'sr25519' });
    const alice = keyring.addFromUri('//Alice');

    await listenOneBlock(api);

    const [authoritySetId, dkgPubKey] = await Promise.all([
        api.query.dkg.authoritySetId(),
        api.query.dkg.dKGPublicKey()
    ]);

    console.log(`DKG authority set id: ${authoritySetId}`);
    console.log(`DKG pub key: ${dkgPubKey}`);

    // const tokenUpdate = api.createType('TokenUpdate', { data: Buffer.from(proposalData).toString('hex') });
    const call = api.tx.dKGProposalHandler.forceSubmitUnsignedProposal({
      TokenUpdate: {
        data: '0x00000000000000000000000000000000000000000001' + 
          '0000000000000000000000000000000000000000000000000000000000000001' +
          '0000000000000000000000000000000000000000000000000000000000000001' +
          '0000000000000000000000000000000000000000000000000000000000000001'
      }
    });
    const unsub = await api.tx.sudo
      .sudo(call)
      .signAndSend(alice, ({ events = [], status }) => {
        console.log(`Current status is: ${status.type}`);

        if (status.isFinalized) {
            console.log(`Transaction included at blockHash ${status.asFinalized}`);

            events.forEach(({ phase, event: { data, method, section } }) => {
                console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
            });

            unsub();
        }
    });
}

// Run
testDirectProposal().catch(console.error).finally(() => process.exit());