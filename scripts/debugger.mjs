#!/usr/bin/env -S node --experimental-modules
// @ts-check
import { spawn, execSync, ChildProcess } from "node:child_process";
import * as readline from "node:readline";
import { stdin as input, stdout as output } from "node:process";
import { inspect } from "node:util";

const authorityMap = {
	alice: {
		ws: 9944,
		http: 9933,
		p2p: 30333,
		color: "green",
	},
	bob: {
		ws: 9945,
		http: 9934,
		p2p: 30334,
		color: "blue",
	},
	charlie: {
		ws: 9946,
		http: 9935,
		p2p: 30335,
		color: "yellow",
	},
};

function buildCompletions() {
	const completions = ["exit", "quit", "q"];
	for (const authority of Object.keys(authorityMap)) {
		completions.push(`start node ${authority}`);
		completions.push(`stop node ${authority}`);
	}
	// another helper for start all nodes
	completions.push("start all");
	completions.push("stop all");
	completions.push("compile");
	return completions;
}

/**
 * @param {string} line
 */
function completer(line) {
	const completions = buildCompletions();
	const hits = completions.filter((c) => c.startsWith(line));
	// Show all completions if none found
	return [hits.length ? hits : completions, line];
}

const rl = readline.createInterface({
	input,
	output,
	completer,
	history: buildCompletions(),
	historySize: 50,
	removeHistoryDuplicates: true,
	prompt: "DKG > ",
	terminal: true,
	tabSize: 4,
});

/**
 * @param {string | Buffer} msg
 */
function print(msg) {
	output.clearLine(0);
	output.cursorTo(0);
	output.write(msg);
	rl.prompt(true);
}

/**
 * @param {string} format
 * @param {string} text
 * @returns {string}
 */
function colorText(format, text) {
	const formatCodes = inspect.colors[format];
	return `\u001b[${formatCodes[0]}m${text}\u001b[${formatCodes[1]}m`;
}

/**
 * @param {'alice' | 'bob' | 'charlie'} authority
 * @returns {ChildProcess}
 */
function startNode(authority) {
	const opts = authorityMap[authority];
	if (opts === undefined) {
		throw new Error(`Unknown authority: ${authority}`);
	}
	const startArgs = [
		"-ldkg=debug",
		"-ldkg_metadata=debug",
		"-lruntime::offchain=debug",
		"-ldkg_proposal_handler=debug",
		`--base-path=./tmp/${authority}`,
		"--rpc-cors",
		"all",
		"--rpc-methods=unsafe",
		"--ws-external",
		`--ws-port=${opts.ws}`,
		`--rpc-port=${opts.http}`,
		`--port=${opts.p2p}`,
		`--${authority}`,
	];

	// get git root
	const gitRoot = execSync("git rev-parse --show-toplevel").toString().trim();
	const nodePath = `${gitRoot}/target/release/dkg-standalone-node`;
	const proc = spawn(nodePath, startArgs, { env: { RUST_LOG_STYLE: "always" } });
	const printData = function(/** @type {Buffer} */ data) {
		for (const line of data.toString().trim().split("\n")) {
			// skip empty lines
			if (line.length === 0) {
				return;
			}
			print(`${colorText(opts.color, authority.toUpperCase())}: ${line}\n`);
		}
	};
	proc.stdout.on("data", (data) => {
		printData(data);
	});
	proc.stderr.on("data", (data) => {
		printData(data);
	});

	return proc;
}

const handles = [];
// we need to close all processes when we exit
process.on("exit", () => {
	handles.forEach((handle) => {
		handle.proc.kill();
	});
});
rl.prompt(true);
rl.on("line", (line) => {
	rl.prompt(true);
	const cmd = line.trim();
	if (cmd.match(/^q|exit|quit$/i)) {
		rl.close();
		process.exit(0);
	}
	if (cmd.match(/^help$/i)) {
		const completions = buildCompletions();
		print(`Available commands: ${completions.join(", ")}\n`);
		return;
	}
	// a regex command to match "start node <authority>" and extract the authority
	const startNodeRegex = /^start node (alice|bob|charlie)$/i;
	if (cmd.match(startNodeRegex)) {
		const authority = cmd.match(startNodeRegex)[1].toLowerCase();
		// @ts-ignore
		handles.push({ proc: startNode(authority), authority });
		return;
	}
	// another regex command to match "stop node <authority>" and extract the authority
	const stopNodeRegex = /^stop node (alice|bob|charlie)$/i;
	if (cmd.match(stopNodeRegex)) {
		const authority = cmd.match(stopNodeRegex)[1].toLowerCase();
		// @ts-ignore
		handles.forEach((handle) => {
			if (handle.authority === authority) {
				handle.proc.kill();
				print(`DKG: Stopped ${authority}`);
			}
		});
		return;
	}
	// another regex command to match "start all"
	const startAllRegex = /^start all$/i;
	if (cmd.match(startAllRegex)) {
		Object.keys(authorityMap).forEach((authority) => {
			// @ts-ignore
			handles.push({ proc: startNode(authority), authority });
		});
		return;
	}

	const stopAllRegex = /^stop all$/i;
	if (cmd.match(stopAllRegex)) {
		handles.forEach((handle) => {
			handle.proc.kill();
		});
		return;
	}

	const compileRegex = /^compile$/i;
	if (cmd.match(compileRegex)) {
		// invoke cargo b -q --release
		const cargo = spawn("cargo", ["b", "--release"]);
		cargo.stdout.on("data", (data) => {
			print(data.toString());
		});
		cargo.stderr.on("data", (data) => {
			print(data.toString());
		});
		cargo.on("exit", (code) => {
			if (code === 0) {
				print("DKG: Compiled");
			} else {
				print("DKG: Compile failed");
			}
		});
		return;
	}
	print(`DKG: Unknown command: ${cmd}`);
	print(`Try "help" for the list of commands.`);
});
