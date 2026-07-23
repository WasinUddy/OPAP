#!/usr/bin/env node

import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import {
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { fileURLToPath } from "node:url";

import { readContainedText } from "./generate-third-party-licenses.mjs";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = resolve(dirname(SCRIPT_PATH), "../..");
const CONFIG_PATH = "apps/desktop/src-tauri/tauri.conf.json";
const REQUIRED_LEGAL_DESTINATIONS = [
  "ASSETS.md",
  "COPYING",
  "NOTICE.md",
  "OSCAR_PROVENANCE.md",
  "THIRD_PARTY_LICENSES.txt",
];

function isInside(child, parent) {
  const pathFromParent = relative(parent, child);
  return (
    pathFromParent === "" ||
    (!pathFromParent.startsWith(`..${sep}`) &&
      pathFromParent !== ".." &&
      !isAbsolute(pathFromParent))
  );
}

export async function verifyTauriLegalResources(
  repositoryRoot = REPOSITORY_ROOT,
) {
  const configSource = await readContainedText(
    repositoryRoot,
    CONFIG_PATH,
    "Tauri configuration",
  );
  const config = JSON.parse(configSource.text);
  const resources = config.bundle?.resources;
  if (!resources || Array.isArray(resources) || typeof resources !== "object") {
    throw new Error("Tauri bundle resources must use an explicit source map");
  }

  const configDirectory = dirname(resolve(repositoryRoot, CONFIG_PATH));
  const staged = new Map();
  const stageRoot = await mkdtemp(join(tmpdir(), "opap-tauri-resources-"));
  try {
    for (const [sourcePath, destinationPath] of Object.entries(resources)) {
      if (
        typeof destinationPath !== "string" ||
        destinationPath.length === 0 ||
        isAbsolute(destinationPath)
      ) {
        throw new Error(`unsafe Tauri resource destination ${destinationPath}`);
      }
      const destination = resolve(stageRoot, destinationPath);
      if (!isInside(destination, stageRoot)) {
        throw new Error(
          `Tauri resource destination escapes bundle: ${destinationPath}`,
        );
      }

      const source = resolve(configDirectory, sourcePath);
      if (!isInside(source, repositoryRoot)) {
        throw new Error(`Tauri resource source escapes repository: ${sourcePath}`);
      }
      const repositoryRelativeSource = relative(repositoryRoot, source);
      const checkedSource = await readContainedText(
        repositoryRoot,
        repositoryRelativeSource,
        `Tauri resource ${destinationPath}`,
      );
      await mkdir(dirname(destination), { recursive: true });
      await writeFile(destination, checkedSource.buffer);
      const stagedBytes = await readFile(destination);
      if (!stagedBytes.equals(checkedSource.buffer) || stagedBytes.length === 0) {
        throw new Error(`Tauri resource staging mismatch: ${destinationPath}`);
      }
      staged.set(destinationPath, {
        sourcePath: repositoryRelativeSource,
        size: stagedBytes.length,
      });
    }

    for (const destination of REQUIRED_LEGAL_DESTINATIONS) {
      if (!staged.has(destination)) {
        throw new Error(`required Tauri legal resource is missing: ${destination}`);
      }
    }
  } finally {
    await rm(stageRoot, { recursive: true, force: true });
  }
  return staged;
}

if (resolve(process.argv[1] ?? "") === SCRIPT_PATH) {
  verifyTauriLegalResources()
    .then((staged) => {
      console.log(
        `Verified ${staged.size} Tauri resources, including all required legal notices.`,
      );
    })
    .catch((error) => {
      console.error(`Tauri legal resource verification failed: ${error.message}`);
      process.exitCode = 1;
    });
}
