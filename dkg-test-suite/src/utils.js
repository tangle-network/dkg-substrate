
exports.hexToBytes = function (hex) {
    for (var bytes = [], c = 0; c < hex.length; c += 2) {
        bytes.push(parseInt(hex.substr(c, 2), 16));
    }
    return bytes;
}

exports.listenOneBlock = async function (api) {
    const unsubscribe = await api.rpc.chain.subscribeNewHeads((header) => {
        console.log(`Chain is at block: #${header.hash}`);
        unsubscribe();
    });
}

exports.printValidators = async function (api) {
    const [{ nonce: accountNonce }, now, validators] = await Promise.all([
        api.query.system.account(ALICE),
        api.query.timestamp.now(),
        api.query.session.validators()
    ]);

    console.log(`accountNonce(${ALICE}) ${accountNonce}`);
    console.log(`last block timestamp ${now.toNumber()}`);

    if (validators && validators.length > 0) {
        const validatorBalances = await Promise.all(
            validators.map((authorityId) =>
                api.query.system.account(authorityId)
            )
        );

        console.log('validators', validators.map((authorityId, index) => ({
            address: authorityId.toString(),
            balance: validatorBalances[index].data.free.toHuman(),
            nonce: validatorBalances[index].nonce.toHuman()
        })));
    }
}