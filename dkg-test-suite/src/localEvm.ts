import ganache from 'ganache';
import { ethers } from 'ethers';
import { Server } from 'ganache';
import { Bridges } from '@webb-tools/protocol-solidity';
import { BridgeInput, DeployerConfig, GovernorConfig } from '@webb-tools/interfaces';
import { MintableToken } from '@webb-tools/tokens';
import { fetchComponentsFromFilePaths } from '@webb-tools/utils';
import path from 'path';
import child from 'child_process';

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
		intinalGovernors: GovernorConfig
	): Promise<Bridges.SignatureBridge> {
		const gitRoot = child.execSync('git rev-parse --show-toplevel').toString().trim();
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
			intinalGovernors,
			zkComponents
		);
	}
}
