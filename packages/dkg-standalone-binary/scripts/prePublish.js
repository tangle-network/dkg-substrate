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
 */

//@ts-check
import { spawnSync, execSync } from "node:child_process";
import { join } from "node:path";
import { copyFileSync } from "node:fs";
import PackageJson from "@npmcli/package-json";
import isCI from "is-ci";

const CRATE_NAME = "dkg-standalone-node";

async function packageJsonInfo() {
	const packageInfo = await PackageJson.load("./");
	return packageInfo.content;
}

async function packageNameWithoutOrgnization() {
	const packageInfo = await PackageJson.load("./");
	return packageInfo.content.name.replace(/^@[^/]+\//, "");
}

/// A function which will start building the dkg-standalone-node binary.
function build() {
	spawnSync("cargo", ["build", "--release", "-p", CRATE_NAME], { stdio: "inherit" });
}

/// Copies the CRATE_NAME binary to the bin directory.
async function copyBinary() {
	const gitRoot = execSync("git rev-parse --show-toplevel").toString().trim();
	const packageName = await packageNameWithoutOrgnization();
	const srcFile = join(gitRoot, `target/release/${CRATE_NAME}`);
	const destFile = join(gitRoot, "packages", packageName, `bin/${CRATE_NAME}`);
	copyFileSync(srcFile, destFile);
}

async function bumpVersionAndPush() {
	execSync(`yarn version --non-interactive --no-git-tag-version --patch`, {
		stdio: "inherit",
	});
	if (isCI) {
		// setup git
		execSync("git config push.default simple");
		execSync("git config merge.ours.driver true");
		execSync('git config user.name "Github Actions"');
		execSync('git config user.email "action@github.com"');

		const pkg = await packageJsonInfo();
		// add and commit
		execSync("git add --all .");
		// add the skip checks for GitHub ...
		execSync(
			`git commit --no-status --quiet -m "[CI Skip] release/${
				pkg.version.includes("-") ? "beta" : "stable"
			} ${pkg.name} ${pkg.version} skip-checks: true"`
		);
		// get current repo remote url
		const remoteUrl = execSync("git config --get remote.origin.url").toString().trim();
		execSync(`git push ${remoteUrl} HEAD:${process.env.GITHUB_REF}`);
	}
}

async function main() {
	try {
		build();
		await copyBinary();
		await bumpVersionAndPush();
	} catch (e) {
		console.error(e);
		process.exit(1);
	}
}

// Run the script main function.
main().catch(console.error);
