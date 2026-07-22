#!/usr/bin/env node

import { opendir, readFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const allowedLicenses = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BlueOak-1.0.0",
  "CC-BY-4.0",
  "CC0-1.0",
  "GPL-3.0-only",
  "GPL-3.0-or-later",
  "ISC",
  "LGPL-2.1-only",
  "LGPL-2.1-or-later",
  "LGPL-3.0-only",
  "LGPL-3.0-or-later",
  "MIT",
  "MIT-0",
  "MPL-2.0",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "Unlicense",
  "Zlib",
]);
const allowedLicenseExceptions = new Set(["Apache-2.0 WITH LLVM-exception"]);

const dependencyRoot = resolve(process.argv[2] ?? "apps/desktop/node_modules");
const packages = new Map();
const violations = [];

function licenseIdentifiers(expression) {
  return expression
    .replace(/\bWITH\s+[A-Za-z0-9.-]+/g, "")
    .match(/[A-Za-z0-9][A-Za-z0-9.-]*/g)
    ?.filter((token) => token !== "AND" && token !== "OR") ?? [];
}

function inspectLicense(packageId, expression) {
  const exceptionClauses = [
    ...expression.matchAll(
      /\b([A-Za-z0-9][A-Za-z0-9.-]*)\s+WITH\s+([A-Za-z0-9][A-Za-z0-9.-]*)\b/g,
    ),
  ].map((match) => `${match[1]} WITH ${match[2]}`);
  const exceptionKeywordCount = expression.match(/\bWITH\b/g)?.length ?? 0;
  const disallowedExceptions = exceptionClauses.filter(
    (clause) => !allowedLicenseExceptions.has(clause),
  );

  if (
    exceptionClauses.length !== exceptionKeywordCount ||
    disallowedExceptions.length > 0
  ) {
    violations.push(
      `${packageId}: ${expression} (disallowed or malformed SPDX exception)`,
    );
    return;
  }

  const identifiers = licenseIdentifiers(expression);

  if (!expression || identifiers.length === 0) {
    violations.push(`${packageId}: missing SPDX license expression`);
    return;
  }

  const disallowed = identifiers.filter((license) => !allowedLicenses.has(license));
  if (disallowed.length > 0) {
    violations.push(`${packageId}: ${expression} (disallowed: ${disallowed.join(", ")})`);
  }
}

async function inspectPackage(packageJsonPath) {
  let manifest;
  try {
    manifest = JSON.parse(await readFile(packageJsonPath, "utf8"));
  } catch (error) {
    violations.push(`${packageJsonPath}: invalid package manifest (${error.message})`);
    return;
  }

  if (!manifest.name || !manifest.version) return;

  const packageId = `${manifest.name}@${manifest.version}`;
  if (packages.has(packageId)) return;
  packages.set(packageId, packageJsonPath);

  const expression =
    typeof manifest.license === "string"
      ? manifest.license.trim()
      : typeof manifest.licenses?.[0]?.type === "string"
        ? manifest.licenses.map((entry) => entry.type).join(" OR ")
        : "";
  inspectLicense(packageId, expression);
}

async function walk(directory) {
  let entries;
  try {
    entries = await opendir(directory);
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new Error(`dependency directory does not exist: ${directory}`);
    }
    throw error;
  }

  for await (const entry of entries) {
    if (entry.name === ".bin") continue;

    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      await walk(path);
    } else if (entry.isFile() && entry.name === "package.json") {
      await inspectPackage(path);
    }
  }
}

if (process.argv[2] === "--pnpm-json") {
  let input = "";
  for await (const chunk of process.stdin) input += chunk;

  const report = JSON.parse(input);
  for (const [expression, entries] of Object.entries(report)) {
    for (const entry of entries) {
      const versions = Array.isArray(entry.versions) ? entry.versions : ["unknown"];
      for (const version of versions) {
        const packageId = `${entry.name ?? "unknown"}@${version}`;
        if (packages.has(packageId)) continue;
        packages.set(packageId, "pnpm license report");
        inspectLicense(packageId, expression);
      }
    }
  }
} else {
  await walk(dependencyRoot);
}

if (packages.size === 0) {
  throw new Error(`no dependency package manifests found under ${dependencyRoot}`);
}

if (violations.length > 0) {
  console.error("Dependency license policy violations:\n");
  for (const violation of violations.sort()) console.error(`- ${violation}`);
  process.exitCode = 1;
} else {
  console.log(`Checked ${packages.size} unique dependency package licenses.`);
}
