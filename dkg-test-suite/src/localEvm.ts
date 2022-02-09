import ganache from 'ganache';
import { ethers } from 'ethers';
import { Server } from 'ganache';
import { Bridge } from '@webb-tools/fixed-bridge';
import { SignatureBridge } from '@webb-tools/fixed-bridge/lib/packages/fixed-bridge/src/SignatureBridge';
import { SignatureBridge as SignatureBridgeContract } from '@webb-tools/contracts/lib/SignatureBridge';
import { MintableToken } from '@webb-tools/tokens';
import { fetchComponentsFromFilePaths } from '@webb-tools/utils';
import path from 'path';
import child from 'child_process';

export type GanacheAccounts = {
	balance: string;
	secretKey: string;
};

export async function startGanacheServer(
	port: number,
	networkId: number,
	populatedAccounts: GanacheAccounts[],
	options: any = {}
): Promise<Server<'ethereum'>> {
	const ganacheServer = ganache.server({
		accounts: populatedAccounts,
		blockTime: 1,
		quiet: true,
		network_id: networkId,
		chainId: networkId,
		...options,
	});

	await ganacheServer.listen(port);
	console.log(`Ganache Started on http://127.0.0.1:${port} ..`);

	return ganacheServer;
}

export class LocalChain {
	public readonly endpoint: string;
	private readonly server: any;
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
		this.server.close();
	}

	public async deployToken(
		name: string,
		symbol: string,
		wallet: ethers.Signer
	): Promise<MintableToken> {
		return MintableToken.createToken(name, symbol, wallet);
	}

	public async deployBridge(
		otherChain: LocalChain,
		localToken: MintableToken,
		otherToken: MintableToken,
		localWallet: ethers.Signer,
		otherWallet: ethers.Signer
	): Promise<Bridge> {
		const gitRoot = child.execSync('git rev-parse --show-toplevel').toString().trim();
		localWallet.connect(this.provider());
		otherWallet.connect(otherChain.provider());
		const bridgeInput = {
			anchorInputs: {
				asset: {
					[this.chainId]: [localToken.contract.address],
					[otherChain.chainId]: [otherToken.contract.address],
				},
				anchorSizes: [ethers.utils.parseEther('1')],
			},
			chainIDs: [this.chainId, otherChain.chainId],
		};
		const deployerConfig = {
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
			path.resolve(__dirname, '../protocol-solidity-fixtures/fixtures/bridge/2/circuit_final.zkey')
		);

		return Bridge.deployBridge(bridgeInput, deployerConfig, zkComponents);
	}

	public async deploySignatureBridge(
		otherChain: LocalChain,
		localToken: MintableToken,
		otherToken: MintableToken,
		localWallet: ethers.Signer,
		otherWallet: ethers.Signer
	): Promise<SignatureBridge> {
		const gitRoot = child.execSync('git rev-parse --show-toplevel').toString().trim();
		localWallet.connect(this.provider());
		otherWallet.connect(otherChain.provider());
		const bridgeInput = {
			anchorInputs: {
				asset: {
					[this.chainId]: [localToken.contract.address],
					[otherChain.chainId]: [otherToken.contract.address],
				},
				anchorSizes: [ethers.utils.parseEther('1')],
			},
			chainIDs: [this.chainId, otherChain.chainId],
		};
		const deployerConfig = {
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

		return SignatureBridge.deployBridge(bridgeInput, deployerConfig, zkComponents);
	}
}
