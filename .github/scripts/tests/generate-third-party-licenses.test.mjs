import assert from "node:assert/strict";
import {
  mkdir,
  mkdtemp,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  assertDistributionReady,
  collectInstalledComponent,
  discoverLicenseDocuments,
  readContainedText,
  reachablePackageIds,
  renderNotice,
  writeNoticeOutput,
} from "../generate-third-party-licenses.mjs";
import {
  safelyRecreateTargetDirectory,
} from "../prepare-third-party-node-dependencies.mjs";

const TEST_ROOT = dirname(fileURLToPath(import.meta.url));
const FIXTURE_ROOT = join(TEST_ROOT, "fixtures");
const REPOSITORY_ROOT = resolve(TEST_ROOT, "../../..");

async function fixtureManifest(name) {
  return JSON.parse(
    await readFile(join(FIXTURE_ROOT, name, "package.json"), "utf8"),
  );
}

test("renders deterministically regardless of component order", async () => {
  const packageRoot = join(FIXTURE_ROOT, "multi-license");
  const component = await collectInstalledComponent({
    ecosystem: "npm",
    manifest: await fixtureManifest("multi-license"),
    packageRoot,
    inventory: "fixture inventory",
  });
  const second = {
    ...component,
    name: "another-fixture",
    version: "9.8.7",
    documents: [...component.documents].reverse(),
  };
  const locks = [
    { path: "z.lock", hash: "2".repeat(64) },
    { path: "a.lock", hash: "1".repeat(64) },
  ];

  const forward = renderNotice({
    components: [component, second],
    locks,
    repositoryRoot: resolve(TEST_ROOT, "../../../../../"),
  });
  const reverse = renderNotice({
    components: [second, component],
    locks: [...locks].reverse(),
    repositoryRoot: resolve(TEST_ROOT, "../../../../../"),
  });
  assert.equal(forward, reverse);
});

test("does not disclose installed absolute paths", async () => {
  const packageRoot = resolve(FIXTURE_ROOT, "multi-license");
  const component = await collectInstalledComponent({
    ecosystem: "npm",
    manifest: await fixtureManifest("multi-license"),
    packageRoot,
    inventory: "fixture inventory",
  });
  const output = renderNotice({
    components: [component],
    locks: [{ path: "fixture.lock", hash: "a".repeat(64) }],
    localPaths: [packageRoot, FIXTURE_ROOT],
  });

  assert.equal(output.includes(packageRoot), false);
  assert.equal(output.includes(FIXTURE_ROOT), false);
});

test("fails closed when installed license text is missing", async () => {
  await assert.rejects(
    discoverLicenseDocuments(join(FIXTURE_ROOT, "missing-text"), {
      key: "npm:fixture-missing-text@4.5.6",
    }),
    /no complete installed LICENSE/,
  );
});

test("pointer-only license files require supplemental complete terms", async () => {
  const packageRoot = join(FIXTURE_ROOT, "pointer-only");
  const manifest = await fixtureManifest("pointer-only");

  await assert.rejects(
    collectInstalledComponent({
      ecosystem: "npm",
      manifest,
      packageRoot,
      inventory: "fixture inventory",
    }),
    /no complete installed LICENSE/,
  );

  const discovered = await discoverLicenseDocuments(packageRoot, {
    key: "npm:fixture-pointer-only@1.2.3",
    allowMissing: true,
  });
  assert.equal(discovered.length, 1);
  assert.equal(discovered[0].classification, "installed-license-pointer");

  const supplementalText =
    "Permission is hereby granted, free of charge, to any person obtaining a copy.\n";
  const component = await collectInstalledComponent({
    ecosystem: "npm",
    manifest,
    packageRoot,
    inventory: "fixture inventory",
    supplement: {
      documents: [
        {
          fileName: "pinned complete terms",
          classification: "license-terms",
          hash: "d".repeat(64),
          text: supplementalText,
        },
      ],
      releaseEvidence: "fixture release evidence",
      unresolvedMissingTextReason: "",
    },
  });
  assert.deepEqual(
    component.documents.map((document) => document.classification).sort(),
    ["installed-license-pointer", "license-terms"],
  );
});

test("bare SPDX-expression files do not count as complete license terms", async () => {
  const packageRoot = join(FIXTURE_ROOT, "bare-expression");
  const manifest = await fixtureManifest("bare-expression");

  await assert.rejects(
    collectInstalledComponent({
      ecosystem: "npm",
      manifest,
      packageRoot,
      inventory: "fixture inventory",
    }),
    /no complete installed LICENSE/,
  );
  const discovered = await discoverLicenseDocuments(packageRoot, {
    key: "npm:fixture-bare-expression@2.3.4",
    allowMissing: true,
  });
  assert.equal(discovered.length, 1);
  assert.equal(discovered[0].classification, "installed-license-pointer");
});

test("preserves multi-license expressions and all discovered texts", async () => {
  const packageRoot = join(FIXTURE_ROOT, "multi-license");
  const component = await collectInstalledComponent({
    ecosystem: "npm",
    manifest: await fixtureManifest("multi-license"),
    packageRoot,
    inventory: "fixture inventory",
  });

  assert.equal(component.licenseExpression, "(MIT OR Apache-2.0)");
  assert.deepEqual(
    component.documents.map((document) => document.fileName).sort(),
    ["LICENSE-APACHE", "LICENSE-MIT"],
  );
  const output = renderNotice({
    components: [component],
    locks: [{ path: "fixture.lock", hash: "b".repeat(64) }],
  });
  assert.match(output, /Apache fixture license text/);
  assert.match(output, /MIT fixture license text/);
});

test("rejects a README symlink that escapes the installed package", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "opap-license-symlink-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const packageRoot = join(root, "package");
  await mkdir(packageRoot);
  await writeFile(
    join(root, "outside-readme.md"),
    "# License\n\nPermission is hereby granted.\nTHE SOFTWARE IS PROVIDED.\n",
  );
  await symlink("../outside-readme.md", join(packageRoot, "README.md"));

  await assert.rejects(
    discoverLicenseDocuments(packageRoot, {
      key: "npm:escaped-readme@1.0.0",
    }),
    /symlink escapes its trusted root/,
  );
});

test("rejects an oversized README before scanning its license section", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "opap-license-oversize-readme-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(join(root, "README.md"), Buffer.alloc(2 * 1024 * 1024 + 1, 65));

  await assert.rejects(
    discoverLicenseDocuments(root, {
      key: "npm:oversized-readme@1.0.0",
    }),
    /text file exceeds 2 MiB/,
  );
});

test("applies size and NUL checks to supplemental text reads", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "opap-license-supplement-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(
    join(root, "oversized.txt"),
    Buffer.alloc(2 * 1024 * 1024 + 1, 65),
  );
  await writeFile(join(root, "binary.txt"), Buffer.from("MIT\0terms"));

  await assert.rejects(
    readContainedText(root, "oversized.txt", "test supplement"),
    /text file exceeds 2 MiB/,
  );
  await assert.rejects(
    readContainedText(root, "binary.txt", "test supplement"),
    /contains NUL bytes/,
  );
});

test("renders unresolved packages explicitly and blocks distribution", async () => {
  const packageRoot = join(FIXTURE_ROOT, "multi-license");
  const component = await collectInstalledComponent({
    ecosystem: "npm",
    manifest: await fixtureManifest("multi-license"),
    packageRoot,
    inventory: "fixture inventory",
  });
  component.missingTextReason =
    "The exact package declares MIT but omits independently verifiable terms.";

  const output = renderNotice({
    components: [component],
    locks: [{ path: "fixture.lock", hash: "c".repeat(64) }],
  });
  assert.match(output, /Unresolved missing-text exceptions: 1/);
  assert.match(output, /DISTRIBUTION BLOCKED/);
  assert.match(output, /independently verifiable terms/);
  assert.throws(
    () => assertDistributionReady([component]),
    /distribution is blocked by 1 unresolved/,
  );
});

test("node distribution targets exclude dev-only test packages", async () => {
  const policy = JSON.parse(
    await readFile(
      join(REPOSITORY_ROOT, ".github/scripts/third-party-targets.json"),
      "utf8",
    ),
  );
  assert.equal(policy.nodeDependencyScope, "production");
  assert.deepEqual(policy.excludedDevOnlyNodePackages, ["stackback", "vitest"]);
});

test("Cargo reachability excludes exclusively-dev dependency edges", () => {
  const metadata = {
    workspace_members: ["root"],
    resolve: {
      nodes: [
        {
          id: "root",
          deps: [
            {
              pkg: "runtime",
              dep_kinds: [{ kind: null, target: null }],
            },
            {
              pkg: "build",
              dep_kinds: [{ kind: "build", target: null }],
            },
            {
              pkg: "dev-only",
              dep_kinds: [{ kind: "dev", target: null }],
            },
            {
              pkg: "mixed",
              dep_kinds: [
                { kind: "dev", target: null },
                { kind: null, target: null },
              ],
            },
            {
              pkg: "legacy-shape",
            },
          ],
        },
        {
          id: "runtime",
          deps: [
            {
              pkg: "runtime-transitive",
              dep_kinds: [{ kind: null, target: null }],
            },
            {
              pkg: "runtime-dev-only",
              dep_kinds: [{ kind: "dev", target: null }],
            },
          ],
        },
        { id: "build", deps: [] },
        { id: "dev-only", deps: [] },
        { id: "mixed", deps: [] },
        { id: "legacy-shape", deps: [] },
        { id: "runtime-transitive", deps: [] },
        {
          id: "runtime-dev-only",
          deps: [
            {
              pkg: "hidden-behind-dev",
              dep_kinds: [{ kind: null, target: null }],
            },
          ],
        },
        { id: "hidden-behind-dev", deps: [] },
      ],
    },
  };

  assert.deepEqual([...reachablePackageIds(metadata)].sort(), [
    "build",
    "legacy-shape",
    "mixed",
    "root",
    "runtime",
    "runtime-transitive",
  ]);
});

test("notice output refuses a symlink without modifying its target", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "opap-notice-output-symlink-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const repositoryRoot = join(root, "repository");
  const outsidePath = join(root, "outside.txt");
  const outputPath = join(repositoryRoot, "THIRD_PARTY_LICENSES.txt");
  await mkdir(repositoryRoot);
  await writeFile(outsidePath, "outside sentinel\n");
  await symlink(outsidePath, outputPath);

  await assert.rejects(
    writeNoticeOutput(repositoryRoot, outputPath, "replacement\n"),
    /output path must not be a symlink/,
  );
  assert.equal(await readFile(outsidePath, "utf8"), "outside sentinel\n");
});

test("target cleanup refuses symlinked parents and preserves outside files", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "opap-notice-cleanup-symlink-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const repositoryRoot = join(root, "repository");
  const repositoryTarget = join(repositoryRoot, "target");
  const outsideInstallRoot = join(root, "outside-install");
  const outsideTargetRoot = join(outsideInstallRoot, "darwin-arm64");
  const sentinelPath = join(outsideTargetRoot, "sentinel.txt");
  await mkdir(repositoryTarget, { recursive: true });
  await mkdir(outsideTargetRoot, { recursive: true });
  await writeFile(sentinelPath, "do not delete\n");
  await symlink(
    outsideInstallRoot,
    join(repositoryTarget, "third-party-node"),
  );

  const installRoot = join(repositoryTarget, "third-party-node");
  await assert.rejects(
    safelyRecreateTargetDirectory(
      repositoryRoot,
      installRoot,
      join(installRoot, "darwin-arm64"),
    ),
    /refusing cleanup through symlink path component third-party-node/,
  );
  assert.equal(await readFile(sentinelPath, "utf8"), "do not delete\n");
});
