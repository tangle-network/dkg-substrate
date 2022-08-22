/*
 * Copyright 2022 Webb Technologies Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
import ganache from 'ganache';
import { ethers } from 'ethers';
import { Server } from 'ganache';
import { Bridges, VBridge } from '@webb-tools/protocol-solidity';
import { VBridgeInput } from '@webb-tools/vbridge';
import {
	BridgeInput,
	DeployerConfig,
	GovernorConfig,
} from '@webb-tools/interfaces';
import { MintableToken, GovernedTokenWrapper } from '@webb-tools/tokens';
import { fetchComponentsFromFilePaths } from '@webb-tools/utils';
import path from 'path';
import child from 'child_process';
import { BLOCK_TIME } from './constants';

export type GanacheAccounts = {
	balance: string;
	secretKey: string;
};

export function startGanacheServer(
	port: number,
	networkId: number,
	populatedAccounts: GanacheAccounts[],
	options: any = {}
): Server<'ethereum'> {
	const ganacheServer = ganache.server({
		accounts: populatedAccounts,
		quiet: true,
		network_id: networkId,
		chainId: networkId,
		...options,
	});

	ganacheServer.listen(port).then(() => {
		process.stdout.write(`Ganache Started on http://127.0.0.1:${port} ..\n`);
	});

	return ganacheServer;
}

export class LocalChain {
	public readonly endpoint: string;
	private readonly server: Server<'ethereum'>;
	constructor(
		public readonly name: string,
		public readonly chainId: number,
		readonly initalBalances: GanacheAccounts[]
	) {
		this.endpoint = `http://localhost:${chainId}`;
		this.server = startGanacheServer(chainId, chainId, initalBalances);
	}

	public provider(): ethers.providers.WebSocketProvider {
		return new ethers.providers.WebSocketProvider(this.endpoint);
	}

	public async stop() {
		await this.server.close();
	}

	public async deployToken(
		name: string,
		symbol: string,
		wallet: ethers.Signer
	): Promise<MintableToken> {
		return MintableToken.createToken(name, symbol, wallet);
	}

	public async deploySignatureBridge(
		otherChain: LocalChain,
		localToken: MintableToken,
		otherToken: MintableToken,
		localWallet: ethers.Signer,
		otherWallet: ethers.Signer,
		initialGovernors: GovernorConfig
	): Promise<Bridges.SignatureBridge> {
		const gitRoot = child
			.execSync('git rev-parse --show-toplevel')
			.toString()
			.trim();
		localWallet.connect(this.provider());
		otherWallet.connect(otherChain.provider());
		const bridgeInput: BridgeInput = {
			anchorInputs: {
				asset: {
					[this.chainId]: [localToken.contract.address],
					[otherChain.chainId]: [otherToken.contract.address],
				},
				anchorSizes: [ethers.utils.parseEther('1')],
			},
			chainIDs: [this.chainId, otherChain.chainId],
		};
		const deployerConfig: DeployerConfig = {
			[this.chainId]: localWallet,
			[otherChain.chainId]: otherWallet,
		};
		const zkComponents = await fetchComponentsFromFilePaths(
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/bridge/2/poseidon_bridge_2.wasm'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/bridge/2/witness_calculator.js'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/bridge/2/circuit_final.zkey'
			)
		);

		return Bridges.SignatureBridge.deployFixedDepositBridge(
			bridgeInput,
			deployerConfig,
			initialGovernors,
			zkComponents
		);
	}

	public async deploySignatureVBridge(
		otherChain: LocalChain,
		localToken: MintableToken,
		otherToken: MintableToken,
		localWallet: ethers.Signer,
		otherWallet: ethers.Signer,
		initialGovernors: GovernorConfig
	): Promise<VBridge.VBridge> {
		const gitRoot = child
			.execSync('git rev-parse --show-toplevel')
			.toString()
			.trim();
		localWallet.connect(this.provider());
		otherWallet.connect(otherChain.provider());
		let webbTokens1 = new Map<number, GovernedTokenWrapper | undefined>();
		webbTokens1.set(this.chainId, null!);
		webbTokens1.set(otherChain.chainId, null!);
		// create the config for the bridge
		const vBridgeInput = {
			vAnchorInputs: {
				asset: {
					[this.chainId]: [localToken.contract.address],
					[otherChain.chainId]: [otherToken.contract.address],
				},
			},
			chainIDs: [this.chainId, otherChain.chainId],
			webbTokens: webbTokens1,
		};

		const deployerConfig: DeployerConfig = {
			[this.chainId]: localWallet,
			[otherChain.chainId]: otherWallet,
		};

		const smallCircuitZkComponents = await fetchComponentsFromFilePaths(
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_2/2/poseidon_vanchor_2_2.wasm'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_2/2/witness_calculator.js'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_2/2/circuit_final.zkey'
			)
		);

		const largeCircuitZkComponents = await fetchComponentsFromFilePaths(
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_16/2/poseidon_vanchor_16_2.wasm'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_16/2/witness_calculator.js'
			),
			path.resolve(
				gitRoot,
				'dkg-test-suite',
				'protocol-solidity-fixtures/fixtures/vanchor_16/2/circuit_final.zkey'
			)
		);

		return VBridge.VBridge.deployVariableAnchorBridge(
			vBridgeInput,
			deployerConfig,
			initialGovernors,
			smallCircuitZkComponents,
			largeCircuitZkComponents
		);
	}
}