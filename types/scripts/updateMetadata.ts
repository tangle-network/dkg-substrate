// Copyright (C) 2020-2022 Acala Foundation.
// SPDX-License-Identifier: Apache-2.0

// Copyright 2022 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0
// This file has been modified by Webb Technologies Inc.

/* eslint-disable @typescript-eslint/no-explicit-any */

const pkg = require('websocket');

const fs = require('fs');

const { w3cwebsocket: WS } = pkg;

const main = (): void => {
  const endpoint = 'ws://localhost:9944';

  console.log('Connecting to ', endpoint);
  const ws = new WS(endpoint);

  ws.onopen = (): void => {
    ws.send('{"id":"1","jsonrpc":"2.0","method":"state_getMetadata","params":[]}');
  };

  ws.onmessage = (msg: any): void => {
    const fullData = JSON.parse(msg.data);
    const metadata = fullData.result;

    fs.writeFileSync('./src/metadata/static-latest.ts', `export default '${metadata}'`);
    fs.writeFileSync('./src/metadata/metadata.json', JSON.stringify(fullData, null, 2));

    console.log('Done');
    process.exit(0);
  };
};

main();
