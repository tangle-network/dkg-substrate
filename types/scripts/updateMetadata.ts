// Copyright (C) 2020-2022 Acala Foundation.
// SPDX-License-Identifier: Apache-2.0

// Copyright 2022 Webb Technologies Inc.
// SPDX-License-Identifier: Apache-2.0
// This file has been modified by Webb Technologies Inc.

/* eslint-disable @typescript-eslint/no-explicit-any */

// Import the API
const fs = require('fs');
import { ApiPromise, WsProvider } from '@polkadot/api';

async function main () {
  // Create connection to websocket
  const wsProvider = new WsProvider('ws://127.0.0.1:9944');
  // Create a new instance of the api
  const api = await ApiPromise.create({ provider: wsProvider });
  // get the chain information
  const metadata = (await api.rpc.state.getMetadata()).toHex();

  let fullData = {
    "jsonrpc": "2.0",
    "result": metadata
  };
  
  fs.writeFileSync('./src/metadata/static-latest.ts', `export default '${metadata}'`);
  fs.writeFileSync('./src/metadata/metadata.json', JSON.stringify(fullData, null, 2));

    console.log('Done');
    process.exit(0);

}

main().catch(console.error).finally(() => process.exit());