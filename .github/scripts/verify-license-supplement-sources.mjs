#!/usr/bin/env node

import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { readContainedText } from "./generate-third-party-licenses.mjs";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = resolve(dirname(SCRIPT_PATH), "../..");
const MANIFEST_PATH = ".github/scripts/license-supplements/manifest.json";

function digest(algorithm, value, encoding = "hex") {
  return createHash(algorithm).update(value).digest(encoding);
}

async function fetchBytes(url, label) {
  const response = await fetch(url, {
    headers: { "user-agent": "OPAP-license-source-verifier" },
    redirect: "follow",
  });
  if (!response.ok) {
    throw new Error(`${label}: ${url} returned HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

const manifestSource = await readContainedText(
  REPOSITORY_ROOT,
  MANIFEST_PATH,
  "license supplement manifest",
);
const manifest = JSON.parse(manifestSource.text);

for (const document of manifest.documents) {
  const remoteBytes = await fetchBytes(
    document.provenance.url,
    `supplement ${document.id}`,
  );
  const remoteHash = digest("sha256", remoteBytes);
  if (remoteHash !== document.expectedSha256) {
    throw new Error(
      `supplement ${document.id}: remote sha256:${remoteHash} does not ` +
        `match expected sha256:${document.expectedSha256}`,
    );
  }
}

const releaseChecks = new Map();
for (const record of manifest.packages) {
  const source = record.release.sourceUrl ?? record.release.repository;
  const revision = record.release.revision;
  releaseChecks.set(`${source}\0${revision}`, { source, revision });
}

for (const { source, revision } of releaseChecks.values()) {
  if (revision.startsWith("sha512-")) {
    const remoteBytes = await fetchBytes(source, "package release artifact");
    const actualIntegrity = `sha512-${digest("sha512", remoteBytes, "base64")}`;
    if (actualIntegrity !== revision) {
      throw new Error(
        `${source}: integrity ${actualIntegrity} does not match ${revision}`,
      );
    }
    continue;
  }

  const repositoryMatch =
    /^https:\/\/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?$/u.exec(source);
  if (!repositoryMatch || !/^[a-f0-9]{40}$/u.test(revision)) {
    throw new Error(`unsupported release provenance: ${source} @ ${revision}`);
  }
  const commitUrl =
    `https://github.com/${repositoryMatch[1]}/${repositoryMatch[2]}/commit/` +
    `${revision}.patch`;
  const response = await fetch(commitUrl, {
    headers: { "user-agent": "OPAP-license-source-verifier" },
    method: "HEAD",
    redirect: "follow",
  });
  if (!response.ok) {
    throw new Error(
      `release ${source} @ ${revision} returned HTTP ${response.status}`,
    );
  }
}

console.log(
  `Verified ${manifest.documents.length} supplemental document source(s) and ` +
    `${releaseChecks.size} unique package release provenance record(s).`,
);
