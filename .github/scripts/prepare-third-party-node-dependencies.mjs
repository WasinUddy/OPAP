#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  copyFile,
  lstat,
  mkdir,
  readFile,
  realpath,
  rm,
  writeFile,
} from "node:fs/promises";
import {
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = resolve(dirname(SCRIPT_PATH), "../..");

function fail(message) {
  throw new Error(`third-party node dependency preparation failed: ${message}`);
}

function isInside(child, parent) {
  const pathFromParent = relative(parent, child);
  return (
    pathFromParent === "" ||
    (!pathFromParent.startsWith(`..${sep}`) &&
      pathFromParent !== ".." &&
      !isAbsolute(pathFromParent))
  );
}

async function lstatIfPresent(path) {
  try {
    return await lstat(path);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

async function assertNonSymlinkDirectoryComponents(
  repositoryRoot,
  candidate,
) {
  const lexicalRepositoryRoot = resolve(repositoryRoot);
  const lexicalCandidate = resolve(candidate);
  if (
    lexicalCandidate === lexicalRepositoryRoot ||
    !isInside(lexicalCandidate, lexicalRepositoryRoot)
  ) {
    fail("cleanup target must be a child of the repository root");
  }

  const repositoryStat = await lstatIfPresent(lexicalRepositoryRoot);
  if (!repositoryStat?.isDirectory() || repositoryStat.isSymbolicLink()) {
    fail("repository root must be an existing non-symlink directory");
  }
  const canonicalRepositoryRoot = await realpath(lexicalRepositoryRoot);
  let current = lexicalRepositoryRoot;
  for (const component of relative(
    lexicalRepositoryRoot,
    lexicalCandidate,
  ).split(sep)) {
    current = join(current, component);
    const currentStat = await lstatIfPresent(current);
    if (!currentStat) return;
    if (currentStat.isSymbolicLink()) {
      fail(`refusing cleanup through symlink path component ${component}`);
    }
    if (!currentStat.isDirectory()) {
      fail(`cleanup path component ${component} is not a directory`);
    }
    const canonicalCurrent = await realpath(current);
    if (!isInside(canonicalCurrent, canonicalRepositoryRoot)) {
      fail("cleanup path resolves outside the repository root");
    }
  }
}

export async function safelyRecreateTargetDirectory(
  repositoryRoot,
  installRoot,
  targetRoot,
) {
  const lexicalRepositoryRoot = resolve(repositoryRoot);
  const lexicalInstallRoot = resolve(installRoot);
  const lexicalTargetRoot = resolve(targetRoot);
  if (
    lexicalInstallRoot === lexicalRepositoryRoot ||
    !isInside(lexicalInstallRoot, lexicalRepositoryRoot) ||
    lexicalTargetRoot === lexicalInstallRoot ||
    !isInside(lexicalTargetRoot, lexicalInstallRoot)
  ) {
    fail("install and target roots must remain nested inside the repository");
  }

  await assertNonSymlinkDirectoryComponents(
    lexicalRepositoryRoot,
    lexicalInstallRoot,
  );
  await mkdir(lexicalInstallRoot, { recursive: true });
  await assertNonSymlinkDirectoryComponents(
    lexicalRepositoryRoot,
    lexicalInstallRoot,
  );
  await assertNonSymlinkDirectoryComponents(
    lexicalRepositoryRoot,
    lexicalTargetRoot,
  );
  await rm(lexicalTargetRoot, { recursive: true, force: true });
  await mkdir(lexicalTargetRoot);
  await assertNonSymlinkDirectoryComponents(
    lexicalRepositoryRoot,
    lexicalTargetRoot,
  );
}

export async function prepareThirdPartyNodeDependencies(
  repositoryRoot = REPOSITORY_ROOT,
) {
  const targetPolicyPath = join(
    repositoryRoot,
    ".github/scripts/third-party-targets.json",
  );
  const desktopRoot = join(repositoryRoot, "apps/desktop");
  const installRoot = join(repositoryRoot, "target/third-party-node");
  const policy = JSON.parse(await readFile(targetPolicyPath, "utf8"));
  if (policy.schemaVersion !== 1 || !Array.isArray(policy.nodeTargets)) {
    fail("unsupported target policy");
  }
  if (policy.nodeDependencyScope !== "production") {
    fail("node notice inventories must remain production-only");
  }

  const workspacePolicy = await readFile(
    join(desktopRoot, "pnpm-workspace.yaml"),
    "utf8",
  );
  if (/^\s*supportedArchitectures\s*:/mu.test(workspacePolicy)) {
    fail(
      "apps/desktop/pnpm-workspace.yaml must remain host-native; " +
        "target architecture policy belongs only in generated notice installs",
    );
  }

  const labels = new Set();
  for (const target of policy.nodeTargets) {
    if (
      typeof target.label !== "string" ||
      !/^[a-z0-9-]+$/u.test(target.label) ||
      typeof target.os !== "string" ||
      typeof target.cpu !== "string" ||
      (target.libc !== undefined && typeof target.libc !== "string")
    ) {
      fail("each node target must have a safe label, os, cpu, and optional libc");
    }
    if (labels.has(target.label)) fail(`duplicate target label ${target.label}`);
    labels.add(target.label);

    const targetRoot = join(installRoot, target.label);
    await safelyRecreateTargetDirectory(
      repositoryRoot,
      installRoot,
      targetRoot,
    );
    for (const fileName of ["package.json", "pnpm-lock.yaml"]) {
      await copyFile(join(desktopRoot, fileName), join(targetRoot, fileName));
    }
    const targetWorkspacePolicy = [
      workspacePolicy.trimEnd(),
      "",
      "# Generated only for deterministic third-party notice inventory.",
      "supportedArchitectures:",
      "  os:",
      `    - ${target.os}`,
      "  cpu:",
      `    - ${target.cpu}`,
      ...(target.libc ? ["  libc:", `    - ${target.libc}`] : []),
      "",
    ].join("\n");
    await writeFile(
      join(targetRoot, "pnpm-workspace.yaml"),
      targetWorkspacePolicy,
      "utf8",
    );

    const args = [
      "--dir",
      targetRoot,
      "install",
      "--prod",
      "--frozen-lockfile",
      "--ignore-scripts",
      "--os",
      target.os,
      "--cpu",
      target.cpu,
    ];
    if (target.libc) args.push("--libc", target.libc);

    const result = spawnSync("pnpm", args, {
      cwd: repositoryRoot,
      stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0) {
      fail(`pnpm install failed for exact target ${target.label}`);
    }
  }

  console.log(
    `Prepared exact pnpm dependency payloads for ${[...labels].join(", ")}.`,
  );
}

if (resolve(process.argv[1] ?? "") === SCRIPT_PATH) {
  prepareThirdPartyNodeDependencies().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
