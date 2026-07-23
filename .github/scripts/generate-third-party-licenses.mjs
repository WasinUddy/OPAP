#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  lstat,
  readdir,
  readFile,
  realpath,
  rename,
  stat,
  unlink,
  writeFile,
} from "node:fs/promises";
import { homedir } from "node:os";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const DEFAULT_REPOSITORY_ROOT = resolve(dirname(SCRIPT_PATH), "../..");
const TARGET_POLICY_PATH = ".github/scripts/third-party-targets.json";
const SUPPLEMENT_MANIFEST_PATH =
  ".github/scripts/license-supplements/manifest.json";
const LICENSE_FILE_PATTERN =
  /^(?:licen[cs]e|copying|notice|copyright)(?:$|[._-])/iu;
const README_FILE_PATTERN = /^readme(?:$|[._-])/iu;
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const MAX_TEXT_FILE_BYTES = 2 * 1024 * 1024;
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });
const COMPLETE_LICENSE_TERMS_PATTERN =
  /(?:Permission is hereby granted|Permission to use, copy, modify, and distribute|Redistribution and use in source and binary forms|TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION|GNU (?:AFFERO )?(?:GENERAL|LESSER GENERAL) PUBLIC LICENSE|Mozilla Public License Version|Eclipse Public License|ISC License|Boost Software License|The origin of this software must not be misrepresented)/iu;
const REFERENCED_LICENSE_FILE_PATTERN =
  /(?:\bLICEN[CS]E[-_.][A-Za-z0-9][A-Za-z0-9._-]*\b|\b(?:see|refer(?:red)? to)\s+(?:the\s+)?(?:LICEN[CS]E|COPYING)\b)/iu;
const REFERENCED_LICENSE_URL_PATTERN =
  /https?:\/\/[^\s>)]*(?:licen[cs]e|licenses|opensource\.org)[^\s>)]*/iu;
const LICENSE_POINTER_LANGUAGE_PATTERN =
  /\b(?:licensed under|available under|see|refer(?:red)? to|at your option)\b/iu;

function codePointCompare(left, right) {
  const leftText = String(left);
  const rightText = String(right);
  if (leftText < rightText) return -1;
  if (leftText > rightText) return 1;
  return 0;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function normalizeText(value) {
  const withoutBom = value.replace(/^\uFEFF/u, "").replace(/\r\n?/gu, "\n");
  return `${withoutBom
    .split("\n")
    .map((line) => line.replace(/[ \t]+$/u, ""))
    .join("\n")
    .trim()}\n`;
}

function normalizeExpression(value) {
  return String(value ?? "")
    .replace(/\s*\/\s*/gu, " OR ")
    .replace(/\s+/gu, " ")
    .trim();
}

function isBareSpdxLicenseDeclaration(text) {
  const lines = text
    .replace(/^\uFEFF/u, "")
    .replace(/\r\n?/gu, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  if (
    /^(?:#{1,6}\s*)?Licen[cs]e(?:\s*\([^)]*\))?\s*#*$/iu.test(
      lines[0] ?? "",
    )
  ) {
    lines.shift();
    if (/^(?:=+|-+)$/u.test(lines[0] ?? "")) lines.shift();
  }
  const candidate = lines.join(" ").trim();
  if (candidate.length === 0 || candidate.length > 256) return false;

  let normalized;
  try {
    normalized = assertSpdxExpression(candidate, "license pointer");
  } catch {
    return false;
  }
  return (
    normalized === "MIT" ||
    ["ISC", "Unlicense", "Zlib"].includes(normalized) ||
    /\b(?:AND|OR|WITH)\b/u.test(normalized) ||
    /[0-9.+-]/u.test(normalized)
  );
}

function isPointerOnlyLicenseText(text) {
  if (isBareSpdxLicenseDeclaration(text)) return true;
  return (
    text.length <= 16 * 1024 &&
    LICENSE_POINTER_LANGUAGE_PATTERN.test(text) &&
    (REFERENCED_LICENSE_FILE_PATTERN.test(text) ||
      REFERENCED_LICENSE_URL_PATTERN.test(text)) &&
    !COMPLETE_LICENSE_TERMS_PATTERN.test(text)
  );
}

function equivalentSupplementExpression(value) {
  const normalized = normalizeExpression(value)
    .replace(/^\((.*)\)$/u, "$1")
    .trim();
  if (/\b(?:AND|WITH)\b/u.test(normalized) || /[()]/u.test(normalized)) {
    return normalized;
  }
  return normalized.split(/\s+OR\s+/u).sort(codePointCompare).join(" OR ");
}

function packageKey(ecosystem, name, version) {
  return `${ecosystem}:${name}@${version}`;
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

async function readableFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

async function lstatIfPresent(path) {
  try {
    return await lstat(path);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

export async function resolveSafeNoticeOutputPath(
  repositoryRoot,
  outputPath,
) {
  const lexicalRepositoryRoot = resolve(repositoryRoot);
  const lexicalOutputPath = resolve(outputPath);
  if (
    lexicalOutputPath === lexicalRepositoryRoot ||
    !isInside(lexicalOutputPath, lexicalRepositoryRoot)
  ) {
    throw new Error("--output must remain inside the repository root");
  }

  const repositoryStat = await lstatIfPresent(lexicalRepositoryRoot);
  if (!repositoryStat?.isDirectory() || repositoryStat.isSymbolicLink()) {
    throw new Error("repository root must be an existing non-symlink directory");
  }
  const canonicalRepositoryRoot = await realpath(lexicalRepositoryRoot);
  const outputParent = dirname(lexicalOutputPath);
  let current = lexicalRepositoryRoot;
  const parentComponents = relative(
    lexicalRepositoryRoot,
    outputParent,
  ).split(sep).filter(Boolean);
  for (const component of parentComponents) {
    current = join(current, component);
    const currentStat = await lstatIfPresent(current);
    if (!currentStat) {
      throw new Error(`--output parent directory is missing: ${component}`);
    }
    if (currentStat.isSymbolicLink()) {
      throw new Error(
        `--output path must not contain symlink component ${component}`,
      );
    }
    if (!currentStat.isDirectory()) {
      throw new Error(`--output path component is not a directory: ${component}`);
    }
    const canonicalCurrent = await realpath(current);
    if (!isInside(canonicalCurrent, canonicalRepositoryRoot)) {
      throw new Error("--output parent resolves outside the repository root");
    }
  }

  const outputStat = await lstatIfPresent(lexicalOutputPath);
  if (outputStat?.isSymbolicLink()) {
    throw new Error("--output path must not be a symlink");
  }
  if (outputStat && !outputStat.isFile()) {
    throw new Error("--output path must be a regular file");
  }
  const canonicalParent = await realpath(outputParent);
  if (!isInside(canonicalParent, canonicalRepositoryRoot)) {
    throw new Error("--output parent resolves outside the repository root");
  }
  return join(canonicalParent, basename(lexicalOutputPath));
}

export async function writeNoticeOutput(repositoryRoot, outputPath, output) {
  const safeOutputPath = await resolveSafeNoticeOutputPath(
    repositoryRoot,
    outputPath,
  );
  const temporaryPath = join(
    dirname(safeOutputPath),
    `.${basename(safeOutputPath)}.${process.pid}.${randomUUID()}.tmp`,
  );
  try {
    await writeFile(temporaryPath, output, {
      encoding: "utf8",
      flag: "wx",
      mode: 0o644,
    });
    await rename(temporaryPath, safeOutputPath);
  } finally {
    try {
      await unlink(temporaryPath);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
}

export async function readContainedText(
  root,
  relativePath,
  key = "text file",
) {
  if (
    typeof relativePath !== "string" ||
    relativePath.length === 0 ||
    isAbsolute(relativePath)
  ) {
    throw new Error(`${key}: path must be relative to its trusted root`);
  }

  const lexicalRoot = resolve(root);
  const lexicalCandidate = resolve(lexicalRoot, relativePath);
  if (!isInside(lexicalCandidate, lexicalRoot)) {
    throw new Error(`${key}: path escapes its trusted root`);
  }

  const canonicalRoot = await realpath(lexicalRoot);
  let canonicalCandidate;
  try {
    canonicalCandidate = await realpath(lexicalCandidate);
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new Error(`${key}: file is missing`);
    }
    throw error;
  }
  if (!isInside(canonicalCandidate, canonicalRoot)) {
    throw new Error(`${key}: symlink escapes its trusted root`);
  }

  const fileStat = await stat(canonicalCandidate);
  if (!fileStat.isFile()) throw new Error(`${key}: path is not a file`);
  if (fileStat.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(`${key}: text file exceeds 2 MiB`);
  }

  const buffer = await readFile(canonicalCandidate);
  if (buffer.includes(0)) throw new Error(`${key}: file contains NUL bytes`);
  let text;
  try {
    text = UTF8_DECODER.decode(buffer);
  } catch {
    throw new Error(`${key}: file is not valid UTF-8 text`);
  }

  return { buffer, text, canonicalPath: canonicalCandidate };
}

function tokenizeSpdx(expression) {
  const tokens = [];
  const matcher =
    /\s*(\(|\)|AND\b|OR\b|WITH\b|[A-Za-z0-9][A-Za-z0-9.+-]*)/gy;
  let offset = 0;

  while (offset < expression.length) {
    matcher.lastIndex = offset;
    const match = matcher.exec(expression);
    if (!match || match.index !== offset) {
      throw new Error(`not an SPDX expression: ${expression}`);
    }
    tokens.push(match[1]);
    offset = matcher.lastIndex;
  }
  return tokens;
}

export function assertSpdxExpression(expression, key) {
  const normalized = normalizeExpression(expression);
  if (
    normalized.length === 0 ||
    /^(?:unknown|unlicensed|none)$/iu.test(normalized) ||
    /^see license in\b/iu.test(normalized)
  ) {
    throw new Error(`${key}: missing SPDX license expression`);
  }

  const tokens = tokenizeSpdx(normalized);
  let cursor = 0;
  const peek = (value) => tokens[cursor] === value;
  const consume = (value) => {
    if (!peek(value)) {
      throw new Error(`${key}: malformed SPDX expression: ${normalized}`);
    }
    cursor += 1;
  };
  const identifier = () => {
    const token = tokens[cursor];
    if (
      !token ||
      ["(", ")", "AND", "OR", "WITH"].includes(token)
    ) {
      throw new Error(`${key}: malformed SPDX expression: ${normalized}`);
    }
    cursor += 1;
  };
  const primary = () => {
    if (peek("(")) {
      consume("(");
      disjunction();
      consume(")");
    } else {
      identifier();
      if (peek("WITH")) {
        consume("WITH");
        identifier();
      }
    }
  };
  const conjunction = () => {
    primary();
    while (peek("AND")) {
      consume("AND");
      primary();
    }
  };
  const disjunction = () => {
    conjunction();
    while (peek("OR")) {
      consume("OR");
      conjunction();
    }
  };

  disjunction();
  if (cursor !== tokens.length) {
    throw new Error(`${key}: malformed SPDX expression: ${normalized}`);
  }
  return normalized;
}

function safeWebValue(value) {
  const candidate =
    typeof value === "string"
      ? value
      : typeof value?.url === "string"
        ? value.url
        : "";
  const normalized = candidate.trim().replace(/^git\+/u, "");
  if (
    /^(?:https?|git|ssh):\/\//u.test(normalized) ||
    /^git@[^:]+:/u.test(normalized)
  ) {
    return normalized;
  }
  return "";
}

export async function discoverLicenseDocuments(
  packageRoot,
  {
    explicitLicenseFile = "",
    key = "unknown package",
    allowMissing = false,
  } = {},
) {
  const candidates = new Set();
  if (explicitLicenseFile) candidates.add(explicitLicenseFile);

  for (const entry of await readdir(packageRoot, { withFileTypes: true })) {
    if (
      (entry.isFile() || entry.isSymbolicLink()) &&
      LICENSE_FILE_PATTERN.test(entry.name)
    ) {
      candidates.add(entry.name);
    }
  }

  const documents = [];
  for (const candidate of [...candidates].sort(codePointCompare)) {
    let source;
    try {
      source = await readContainedText(
        packageRoot,
        candidate,
        `${key}: ${candidate}`,
      );
    } catch (error) {
      if (
        !explicitLicenseFile ||
        candidate === explicitLicenseFile ||
        !/file is missing$/u.test(error.message)
      ) {
        throw error;
      }
      continue;
    }
    const text = normalizeText(source.text);
    if (text.trim().length === 0) {
      throw new Error(`${key}: ${candidate} is empty`);
    }
    documents.push({
      fileName: basename(candidate),
      classification: isPointerOnlyLicenseText(text)
        ? "installed-license-pointer"
        : "installed-license-or-notice",
      hash: sha256(text),
      text,
    });
  }

  if (documents.length === 0) {
    const readmeNames = (await readdir(packageRoot))
      .filter((name) => README_FILE_PATTERN.test(name))
      .sort(codePointCompare);
    for (const readmeName of readmeNames) {
      const source = await readContainedText(
        packageRoot,
        readmeName,
        `${key}: ${readmeName}`,
      );
      const sectionStart = source.text.search(
        /^(?:#{1,6}\s*)?Licen[cs]e(?:\s*\([^)]*\))?\s*$/imu,
      );
      if (sectionStart < 0) continue;
      const section = source.text.slice(sectionStart);
      const containsFullGrant =
        (/Permission is hereby granted/iu.test(section) &&
          /THE SOFTWARE IS PROVIDED/iu.test(section)) ||
        (/Redistribution and use in source and binary forms/iu.test(section) &&
          /THIS SOFTWARE IS PROVIDED/iu.test(section));
      if (!containsFullGrant) continue;
      const text = normalizeText(section);
      documents.push({
        fileName: `${readmeName} (full license section)`,
        classification: "installed-license-or-notice",
        hash: sha256(text),
        text,
      });
    }
  }

  const unique = new Map();
  for (const document of documents) {
    unique.set(`${document.hash}\0${document.fileName}`, document);
  }
  const discovered = [...unique.values()].sort(
    (left, right) =>
      codePointCompare(left.hash, right.hash) ||
      codePointCompare(left.fileName, right.fileName),
  );
  const hasNonPointerText = discovered.some(
    (document) => document.classification !== "installed-license-pointer",
  );
  if ((!hasNonPointerText || discovered.length === 0) && !allowMissing) {
    throw new Error(
      `${key}: no complete installed LICENSE, LICENCE, COPYING, NOTICE, or ` +
        "COPYRIGHT text found",
    );
  }
  return discovered;
}

function extractLeadingComment(text, key) {
  const normalized = text.replace(/^\uFEFF/u, "").replace(/\r\n?/gu, "\n");
  const withoutShebang = normalized.replace(/^#![^\n]*\n/u, "");
  const trimmed = withoutShebang.trimStart();
  if (trimmed.startsWith("/*")) {
    const end = trimmed.indexOf("*/");
    if (end < 0) throw new Error(`${key}: unterminated leading comment`);
    return normalizeText(trimmed.slice(0, end + 2));
  }
  if (trimmed.startsWith("//")) {
    const lines = trimmed.split("\n");
    const selected = [];
    for (const line of lines) {
      if (/^\s*\/\//u.test(line) || (selected.length > 0 && /^\s*$/u.test(line))) {
        selected.push(line);
      } else {
        break;
      }
    }
    while (selected.at(-1)?.trim() === "") selected.pop();
    return normalizeText(selected.join("\n"));
  }
  throw new Error(`${key}: no leading comment was found`);
}

function extractMarkdownSection(text, heading, key) {
  const lines = text.replace(/\r\n?/gu, "\n").split("\n");
  let start = -1;
  let level = 0;
  for (let index = 0; index < lines.length; index += 1) {
    const match = /^(#{1,6})\s+(.+?)\s*#*\s*$/u.exec(lines[index]);
    if (match && match[2].trim().toLowerCase() === heading.toLowerCase()) {
      start = index;
      level = match[1].length;
      break;
    }
    if (
      lines[index].trim().toLowerCase() === heading.toLowerCase() &&
      /^(?:=+|-+)\s*$/u.test(lines[index + 1] ?? "")
    ) {
      start = index;
      level = lines[index + 1].trim().startsWith("=") ? 1 : 2;
      break;
    }
  }
  if (start < 0) throw new Error(`${key}: Markdown heading "${heading}" is missing`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    const match = /^(#{1,6})\s+/u.exec(lines[index]);
    if (match && match[1].length <= level) {
      end = index;
      break;
    }
    if (
      index + 1 < lines.length &&
      /^(?:=+|-+)\s*$/u.test(lines[index + 1]) &&
      (lines[index + 1].trim().startsWith("=") || level >= 2)
    ) {
      end = index;
      break;
    }
  }
  return normalizeText(lines.slice(start, end).join("\n"));
}

function extractJsonFields(text, fields, key) {
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`${key}: invalid JSON evidence: ${error.message}`);
  }
  const selected = {};
  for (const field of fields) {
    if (!(field in parsed)) throw new Error(`${key}: JSON field ${field} is missing`);
    selected[field] = parsed[field];
  }
  return normalizeText(JSON.stringify(selected, null, 2));
}

async function loadTargetPolicy(repositoryRoot) {
  const source = await readContainedText(
    repositoryRoot,
    TARGET_POLICY_PATH,
    "third-party target policy",
  );
  const policy = JSON.parse(source.text);
  if (
    policy.schemaVersion !== 1 ||
    !policy.tools ||
    policy.nodeDependencyScope !== "production" ||
    !Array.isArray(policy.excludedDevOnlyNodePackages) ||
    !Array.isArray(policy.rustInventories) ||
    !Array.isArray(policy.nodeTargets)
  ) {
    throw new Error("third-party target policy has an unsupported schema");
  }
  const targetLabels = new Set();
  for (const target of policy.nodeTargets) {
    if (
      typeof target.label !== "string" ||
      !/^[a-z0-9-]+$/u.test(target.label) ||
      typeof target.os !== "string" ||
      typeof target.cpu !== "string" ||
      (target.libc !== undefined && typeof target.libc !== "string")
    ) {
      throw new Error("third-party target policy contains an invalid node target");
    }
    if (targetLabels.has(target.label)) {
      throw new Error(`third-party target policy repeats ${target.label}`);
    }
    targetLabels.add(target.label);
  }
  return policy;
}

async function loadSupplements(repositoryRoot) {
  const manifestSource = await readContainedText(
    repositoryRoot,
    SUPPLEMENT_MANIFEST_PATH,
    "license supplement manifest",
  );
  const manifest = JSON.parse(manifestSource.text);
  if (
    manifest.schemaVersion !== 1 ||
    !Array.isArray(manifest.documents) ||
    !Array.isArray(manifest.packages)
  ) {
    throw new Error("license supplement manifest has an unsupported schema");
  }

  const documents = new Map();
  for (const record of manifest.documents) {
    if (
      typeof record.id !== "string" ||
      documents.has(record.id) ||
      !SHA256_PATTERN.test(record.expectedSha256 ?? "") ||
      !["license-terms", "notice"].includes(record.classification) ||
      typeof record.provenance?.url !== "string" ||
      !record.provenance.url.startsWith("https://") ||
      typeof record.provenance?.revision !== "string" ||
      record.provenance.revision.length === 0
    ) {
      throw new Error(`invalid supplemental document record ${record.id ?? ""}`);
    }
    const source = await readContainedText(
      repositoryRoot,
      record.localPath,
      `supplement ${record.id}`,
    );
    const rawHash = sha256(source.buffer);
    if (rawHash !== record.expectedSha256) {
      throw new Error(
        `supplement ${record.id}: expected sha256:${record.expectedSha256}, got sha256:${rawHash}`,
      );
    }
    const text = normalizeText(source.text);
    if (text.trim().length === 0) {
      throw new Error(`supplement ${record.id}: text is empty`);
    }
    documents.set(record.id, {
      fileName:
        `pinned supplement ${record.id}; ${record.provenance.url}; ` +
        `revision ${record.provenance.revision}; ` +
        `source sha256:${record.expectedSha256}` +
        (record.provenance.note
          ? `; note: ${record.provenance.note}`
          : ""),
      classification: record.classification,
      hash: sha256(text),
      text,
    });
  }

  const packages = new Map();
  for (const record of manifest.packages) {
    if (
      typeof record.packageKey !== "string" ||
      packages.has(record.packageKey) ||
      !Array.isArray(record.documents) ||
      typeof (record.release?.sourceUrl ?? record.release?.repository) !==
        "string" ||
      typeof record.release?.revision !== "string"
    ) {
      throw new Error(`invalid supplemental package record ${record.packageKey ?? ""}`);
    }
    assertSpdxExpression(
      record.expectedLicenseExpression,
      record.packageKey,
    );
    for (const documentId of record.documents) {
      if (!documents.has(documentId)) {
        throw new Error(
          `${record.packageKey}: unknown supplemental document ${documentId}`,
        );
      }
    }
    packages.set(record.packageKey, record);
  }

  const usedPackages = new Set();
  const usedDocuments = new Set();
  return {
    manifest,
    documents,
    packages,
    async materialize(key, expression, packageRoot) {
      const record = packages.get(key);
      if (!record) {
        return {
          documents: [],
          releaseEvidence: "",
          unresolvedMissingTextReason: "",
        };
      }
      const actual = normalizeExpression(expression);
      const expected = normalizeExpression(record.expectedLicenseExpression);
      if (
        equivalentSupplementExpression(actual) !==
        equivalentSupplementExpression(expected)
      ) {
        throw new Error(
          `${key}: supplemental license expression mismatch; expected ${expected}, got ${actual}`,
        );
      }
      usedPackages.add(key);
      const materialized = record.documents.map((documentId) => {
        usedDocuments.add(documentId);
        return documents.get(documentId);
      });
      for (const evidence of record.installedEvidence ?? []) {
        if (
          typeof evidence.relativePath !== "string" ||
          !SHA256_PATTERN.test(evidence.expectedSourceSha256 ?? "") ||
          typeof evidence.label !== "string"
        ) {
          throw new Error(`${key}: invalid installed evidence record`);
        }
        const source = await readContainedText(
          packageRoot,
          evidence.relativePath,
          `${key}: installed evidence ${evidence.label}`,
        );
        const rawHash = sha256(source.buffer);
        if (rawHash !== evidence.expectedSourceSha256) {
          throw new Error(
            `${key}: ${evidence.label} source hash changed; expected ` +
              `sha256:${evidence.expectedSourceSha256}, got sha256:${rawHash}`,
          );
        }
        let text;
        if (evidence.extraction === "leading-comment") {
          text = extractLeadingComment(source.text, `${key}: ${evidence.label}`);
        } else if (evidence.extraction === "markdown-heading") {
          text = extractMarkdownSection(
            source.text,
            evidence.heading,
            `${key}: ${evidence.label}`,
          );
        } else if (evidence.extraction === "json-fields") {
          text = extractJsonFields(
            source.text,
            evidence.fields ?? [],
            `${key}: ${evidence.label}`,
          );
        } else if (evidence.extraction === "full-text") {
          text = normalizeText(source.text);
        } else {
          throw new Error(`${key}: unsupported evidence extraction`);
        }
        materialized.push({
          fileName:
            `installed evidence: ${evidence.label} ` +
            `(source sha256:${rawHash})`,
          classification: "notice",
          hash: sha256(text),
          text,
        });
      }
      return {
        documents: materialized,
        releaseEvidence:
          `${record.release.sourceUrl ?? record.release.repository} @ ` +
          `${record.release.revision}` +
          (record.release.note ? ` (${record.release.note})` : ""),
        unresolvedMissingTextReason:
          record.unresolvedMissingTextReason ?? "",
      };
    },
    assertAllUsed() {
      const unusedPackages = [...packages.keys()]
        .filter((key) => !usedPackages.has(key))
        .sort(codePointCompare);
      const unusedDocuments = [...documents.keys()]
        .filter((id) => !usedDocuments.has(id))
        .sort(codePointCompare);
      if (unusedPackages.length > 0 || unusedDocuments.length > 0) {
        throw new Error(
          `unused license supplements: packages=[${unusedPackages.join(", ")}], ` +
            `documents=[${unusedDocuments.join(", ")}]`,
        );
      }
    },
  };
}

export async function collectInstalledComponent({
  ecosystem,
  manifest,
  packageRoot,
  expression,
  inventory,
  supplement = {
    documents: [],
    releaseEvidence: "",
    unresolvedMissingTextReason: "",
  },
}) {
  const name = String(manifest.name ?? "").trim();
  const version = String(manifest.version ?? "").trim();
  if (!name || !version) {
    throw new Error(`${packageRoot}: package name and version are required`);
  }
  const key = packageKey(ecosystem, name, version);
  const licenseExpression = assertSpdxExpression(
    expression ?? manifest.license,
    key,
  );
  const installedDocuments = await discoverLicenseDocuments(packageRoot, {
    explicitLicenseFile:
      ecosystem === "cargo" ? manifest.license_file ?? "" : "",
    key,
    allowMissing:
      supplement.documents.length > 0 ||
      Boolean(supplement.unresolvedMissingTextReason),
  });
  const documents = [...installedDocuments, ...supplement.documents];
  if (documents.length === 0) {
    throw new Error(`${key}: no complete license or notice text is available`);
  }
  const hasInstalledCompleteText = installedDocuments.some(
    (document) => document.classification !== "installed-license-pointer",
  );
  const hasSupplementalCompleteTerms = supplement.documents.some(
    (document) => document.classification === "license-terms",
  );
  if (
    !hasInstalledCompleteText &&
    !hasSupplementalCompleteTerms &&
    !supplement.unresolvedMissingTextReason
  ) {
    throw new Error(`${key}: no complete license terms are available`);
  }
  if (
    supplement.unresolvedMissingTextReason &&
    documents.some((document) => document.classification === "license-terms")
  ) {
    throw new Error(
      `${key}: unresolved package must not claim supplemental complete terms`,
    );
  }

  return {
    ecosystem,
    name,
    version,
    licenseExpression,
    homepage: safeWebValue(manifest.homepage),
    repository: safeWebValue(manifest.repository),
    releaseEvidence: supplement.releaseEvidence,
    inventories: [inventory],
    documents,
    missingTextReason: supplement.unresolvedMissingTextReason,
  };
}

function runJsonCommand(command, args, workingDirectory) {
  const result = spawnSync(command, args, {
    cwd: workingDirectory,
    encoding: "utf8",
    maxBuffer: 128 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed:\n${result.stderr.trim()}`,
    );
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`${command} returned invalid JSON: ${error.message}`);
  }
}

export function reachablePackageIds(metadata) {
  if (!metadata.resolve) {
    throw new Error("cargo metadata did not include a dependency resolution");
  }
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const pending = [...metadata.workspace_members];
  const reachable = new Set();
  while (pending.length > 0) {
    const id = pending.pop();
    if (reachable.has(id)) continue;
    reachable.add(id);
    for (const dependency of nodes.get(id)?.deps ?? []) {
      const dependencyKinds = dependency.dep_kinds;
      const isExclusivelyDev =
        Array.isArray(dependencyKinds) &&
        dependencyKinds.length > 0 &&
        dependencyKinds.every((entry) => entry?.kind === "dev");
      if (!isExclusivelyDev) pending.push(dependency.pkg);
    }
  }
  return reachable;
}

async function collectRustComponents(repositoryRoot, policy, supplements) {
  const collected = [];
  for (const inventory of policy.rustInventories) {
    if (
      typeof inventory.label !== "string" ||
      typeof inventory.manifest !== "string" ||
      !Array.isArray(inventory.targets)
    ) {
      throw new Error("third-party target policy contains an invalid Rust inventory");
    }
    for (const target of inventory.targets) {
      const metadata = runJsonCommand(
        "cargo",
        [
          "metadata",
          "--manifest-path",
          inventory.manifest,
          "--all-features",
          "--locked",
          "--offline",
          "--filter-platform",
          target,
          "--format-version",
          "1",
        ],
        repositoryRoot,
      );
      const reachable = reachablePackageIds(metadata);
      for (const manifest of metadata.packages) {
        if (!reachable.has(manifest.id) || manifest.source === null) continue;
        const key = packageKey("cargo", manifest.name, manifest.version);
        const expression = assertSpdxExpression(manifest.license, key);
        const packageRoot = dirname(manifest.manifest_path);
        const supplement = await supplements.materialize(
          key,
          expression,
          packageRoot,
        );
        collected.push(
          await collectInstalledComponent({
            ecosystem: "cargo",
            manifest,
            packageRoot,
            expression,
            inventory: `${inventory.label}; exact target ${target}`,
            supplement,
          }),
        );
      }
    }
  }
  return collected;
}

function matchesPlatformConstraint(constraint, selected) {
  if (!Array.isArray(constraint) || constraint.length === 0) return true;
  const positives = constraint.filter((value) => !value.startsWith("!"));
  const negatives = constraint
    .filter((value) => value.startsWith("!"))
    .map((value) => value.slice(1));
  return (
    !negatives.includes(selected) &&
    (positives.length === 0 || positives.includes(selected))
  );
}

function matchesNodeTarget(manifest, target) {
  return (
    matchesPlatformConstraint(manifest.os, target.os) &&
    matchesPlatformConstraint(manifest.cpu, target.cpu) &&
    matchesPlatformConstraint(manifest.libc, target.libc ?? "")
  );
}

async function findPnpmManifest(entry, version, targetRoot) {
  const canonicalTargetRoot = await realpath(targetRoot);
  const paths = Array.isArray(entry.paths)
    ? [...entry.paths].sort(codePointCompare)
    : [];
  for (const packageRoot of paths) {
    let canonicalPackageRoot;
    try {
      canonicalPackageRoot = await realpath(packageRoot);
    } catch (error) {
      if (error.code === "ENOENT") continue;
      throw error;
    }
    if (!isInside(canonicalPackageRoot, canonicalTargetRoot)) {
      throw new Error(
        `npm:${entry.name}@${version}: pnpm report path escapes exact target root`,
      );
    }
    let source;
    try {
      source = await readContainedText(
        packageRoot,
        "package.json",
        `npm:${entry.name}@${version}: package manifest`,
      );
    } catch (error) {
      if (/file is missing$/u.test(error.message)) continue;
      throw error;
    }
    const manifest = JSON.parse(source.text);
    if (manifest.name === entry.name && String(manifest.version) === version) {
      return { manifest, packageRoot };
    }
  }
  throw new Error(
    `npm:${entry.name}@${version}: pnpm report did not identify an installed package`,
  );
}

async function collectPnpmComponents(repositoryRoot, policy, supplements) {
  const collected = [];
  for (const target of policy.nodeTargets) {
    const targetRoot = join(
      repositoryRoot,
      "target/third-party-node",
      target.label,
    );
    if (!(await readableFile(join(targetRoot, "pnpm-lock.yaml")))) {
      throw new Error(
        `exact pnpm target ${target.label} is not prepared; run ` +
          "node .github/scripts/prepare-third-party-node-dependencies.mjs",
      );
    }
    const report = runJsonCommand(
      "pnpm",
      ["--dir", targetRoot, "licenses", "list", "--prod", "--json"],
      repositoryRoot,
    );
    let targetCount = 0;
    for (const expression of Object.keys(report).sort(codePointCompare)) {
      const entries = Array.isArray(report[expression])
        ? [...report[expression]]
        : [];
      entries.sort(
        (left, right) =>
          codePointCompare(left.name, right.name) ||
          codePointCompare(
            JSON.stringify(left.versions),
            JSON.stringify(right.versions),
          ),
      );
      for (const entry of entries) {
        const versions = Array.isArray(entry.versions)
          ? entry.versions.map(String).sort(codePointCompare)
          : [];
        if (versions.length === 0) {
          throw new Error(`npm:${entry.name}: pnpm report omitted its version`);
        }
        for (const version of versions) {
          const { manifest, packageRoot } = await findPnpmManifest(
            entry,
            version,
            targetRoot,
          );
          const key = packageKey("npm", manifest.name, manifest.version);
          if (policy.excludedDevOnlyNodePackages.includes(manifest.name)) {
            throw new Error(
              `${key}: dev-only package leaked into production target ${target.label}`,
            );
          }
          if (!matchesNodeTarget(manifest, target)) {
            // pnpm can materialize sibling optional libc packages while
            // preparing a foreign Linux target. Only a package whose own
            // published constraints match this exact descriptor belongs to
            // the distribution inventory or receives its target label.
            continue;
          }
          const normalizedExpression = assertSpdxExpression(expression, key);
          const supplement = await supplements.materialize(
            key,
            normalizedExpression,
            packageRoot,
          );
          collected.push(
            await collectInstalledComponent({
              ecosystem: "npm",
              manifest,
              packageRoot,
              expression: normalizedExpression,
              inventory: `Desktop pnpm graph; exact target ${target.label}`,
              supplement,
            }),
          );
          targetCount += 1;
        }
      }
    }
    if (targetCount === 0) {
      throw new Error(
        `pnpm license report for exact target ${target.label} is empty`,
      );
    }
  }
  return collected;
}

function mergeComponents(components) {
  const merged = new Map();
  for (const component of components) {
    const key = packageKey(
      component.ecosystem,
      component.name,
      component.version,
    );
    const existing = merged.get(key);
    if (!existing) {
      merged.set(key, {
        ...component,
        inventories: [...component.inventories],
        documents: [...component.documents],
      });
      continue;
    }
    for (const field of [
      "licenseExpression",
      "homepage",
      "repository",
      "releaseEvidence",
      "missingTextReason",
    ]) {
      if (existing[field] && component[field] && existing[field] !== component[field]) {
        throw new Error(`${key}: conflicting ${field} metadata`);
      }
      if (!existing[field]) existing[field] = component[field];
    }
    existing.inventories.push(...component.inventories);
    existing.documents.push(...component.documents);
  }

  for (const component of merged.values()) {
    component.inventories = [...new Set(component.inventories)].sort(
      codePointCompare,
    );
    component.documents = [
      ...new Map(
        component.documents.map((document) => [
          `${document.hash}\0${document.fileName}`,
          document,
        ]),
      ).values(),
    ].sort(
      (left, right) =>
        codePointCompare(left.hash, right.hash) ||
        codePointCompare(left.fileName, right.fileName),
    );
  }
  return [...merged.values()].sort(
    (left, right) =>
      codePointCompare(left.ecosystem, right.ecosystem) ||
      codePointCompare(left.name, right.name) ||
      codePointCompare(left.version, right.version),
  );
}

async function inputRecords(repositoryRoot, supplementManifest) {
  const inputPaths = [
    "Cargo.lock",
    "Cargo.toml",
    "apps/desktop/package.json",
    "apps/desktop/pnpm-lock.yaml",
    "apps/desktop/pnpm-workspace.yaml",
    "apps/desktop/src-tauri/Cargo.lock",
    "apps/desktop/src-tauri/Cargo.toml",
    ".github/scripts/generate-third-party-licenses.mjs",
    ".github/scripts/prepare-third-party-node-dependencies.mjs",
    ".github/scripts/test-tauri-legal-resources.mjs",
    ".github/scripts/verify-license-supplement-sources.mjs",
    TARGET_POLICY_PATH,
    SUPPLEMENT_MANIFEST_PATH,
    "rust-toolchain.toml",
  ];
  for (const entry of await readdir(join(repositoryRoot, "crates"), {
    withFileTypes: true,
  })) {
    if (entry.isDirectory()) {
      inputPaths.push(`crates/${entry.name}/Cargo.toml`);
    }
  }
  for (const document of supplementManifest.documents) {
    inputPaths.push(document.localPath);
  }

  const records = [];
  for (const path of [...new Set(inputPaths)].sort(codePointCompare)) {
    const source = await readContainedText(
      repositoryRoot,
      path,
      `generation input ${path}`,
    );
    records.push({ path, hash: sha256(source.buffer) });
  }
  return records;
}

function renderComponent(component) {
  const lines = [
    `[${component.ecosystem}] ${component.name} ${component.version}`,
    `SPDX license expression: ${component.licenseExpression}`,
  ];
  if (component.homepage) lines.push(`Homepage: ${component.homepage}`);
  if (component.repository) lines.push(`Repository: ${component.repository}`);
  if (component.releaseEvidence) {
    lines.push(`Pinned release evidence: ${component.releaseEvidence}`);
  }
  lines.push("Included by:");
  for (const inventory of component.inventories) {
    lines.push(`  - ${inventory}`);
  }
  if (component.missingTextReason) {
    lines.push(
      `Unresolved missing-text exception: ${component.missingTextReason}`,
    );
  }
  lines.push("License/notice texts:");
  for (const document of component.documents) {
    lines.push(
      `  - sha256:${document.hash} [${document.classification}] ` +
        `(${document.fileName})`,
    );
  }
  return `${lines.join("\n")}\n`;
}

export function renderNotice({
  components,
  inputs,
  locks,
  policy,
  repositoryRoot = "",
  localPaths = [],
}) {
  const sortedComponents = mergeComponents(components);
  const records = [...(inputs ?? locks ?? [])].sort((left, right) =>
    codePointCompare(left.path, right.path),
  );
  const effectivePolicy = policy ?? {
    tools: {},
    rustInventories: [],
    nodeTargets: [],
  };
  const texts = new Map();
  const references = new Map();
  const unresolvedComponents = sortedComponents.filter(
    (component) => component.missingTextReason,
  );
  for (const component of sortedComponents) {
    const key = packageKey(
      component.ecosystem,
      component.name,
      component.version,
    );
    for (const document of component.documents) {
      const existing = texts.get(document.hash);
      if (existing && existing !== document.text) {
        throw new Error(`SHA-256 collision for ${document.hash}`);
      }
      texts.set(document.hash, document.text);
      const labels = references.get(document.hash) ?? [];
      labels.push(`${key} (${document.fileName})`);
      references.set(document.hash, labels);
    }
  }

  const lines = [
    "OPAP THIRD-PARTY SOFTWARE LICENSES AND NOTICES",
    "================================================",
    "",
    "This file is generated from installed dependency payloads and checked,",
    "version-pinned supplemental source documents. It is an engineering",
    "distribution record, not legal advice or a compliance claim.",
    "",
    "Generation tool policy:",
  ];
  for (const [name, version] of Object.entries(effectivePolicy.tools).sort(
    ([left], [right]) => codePointCompare(left, right),
  )) {
    lines.push(`  - ${name}: ${version}`);
  }
  lines.push("", "Exact distribution targets:");
  for (const inventory of effectivePolicy.rustInventories) {
    for (const target of inventory.targets) {
      lines.push(`  - Cargo: ${inventory.label}; ${target}`);
    }
  }
  for (const target of effectivePolicy.nodeTargets) {
    lines.push(
      `  - pnpm: ${target.label}; os=${target.os}; cpu=${target.cpu}; ` +
        `libc=${target.libc ?? "platform default"}`,
    );
  }
  lines.push(
    `  - pnpm dependency scope: ${effectivePolicy.nodeDependencyScope ?? "unknown"}`,
  );
  lines.push("", "Generation inputs:");
  for (const record of records) {
    lines.push(`  - ${record.path}: sha256:${record.hash}`);
  }
  lines.push(
    "",
    `Components: ${sortedComponents.length}`,
    `Unique license/notice texts: ${texts.size}`,
    `Unresolved missing-text exceptions: ${unresolvedComponents.length}`,
  );
  if (unresolvedComponents.length > 0) {
    lines.push(
      "DISTRIBUTION BLOCKED: resolve every missing-text exception before release.",
      "Unresolved components:",
    );
    for (const component of unresolvedComponents) {
      lines.push(
        `  - ${packageKey(
          component.ecosystem,
          component.name,
          component.version,
        )}`,
      );
    }
  }
  lines.push(
    "",
    "COMPONENT INVENTORY",
    "===================",
    "",
  );
  for (const component of sortedComponents) lines.push(renderComponent(component));
  lines.push(
    "FULL LICENSE AND NOTICE TEXTS",
    "=============================",
    "",
  );
  for (const hash of [...texts.keys()].sort(codePointCompare)) {
    lines.push(`Text SHA-256: ${hash}`, "Referenced by:");
    for (const reference of [...new Set(references.get(hash))].sort(
      codePointCompare,
    )) {
      lines.push(`  - ${reference}`);
    }
    lines.push(
      "",
      "----- BEGIN LICENSE/NOTICE TEXT -----",
      texts.get(hash).trimEnd(),
      "----- END LICENSE/NOTICE TEXT -----",
      "",
    );
  }

  const output = `${lines.join("\n").trimEnd()}\n`;
  const prohibited = [
    repositoryRoot,
    process.cwd(),
    homedir(),
    ...localPaths,
  ].filter((value) => value && isAbsolute(value) && value.length > 1);
  for (const localPath of new Set(prohibited)) {
    if (output.includes(localPath)) {
      throw new Error("generated notice contains an absolute local path");
    }
  }
  if (/\bfile:\/\/(?:\/|[A-Za-z]:)/u.test(output)) {
    throw new Error("generated notice contains a local file URL");
  }
  return output;
}

export function assertDistributionReady(components) {
  const unresolved = components.filter(
    (component) => component.missingTextReason,
  );
  if (unresolved.length > 0) {
    throw new Error(
      `distribution is blocked by ${unresolved.length} unresolved ` +
        "missing-text exception(s)",
    );
  }
}

export async function generateNotice(repositoryRoot = DEFAULT_REPOSITORY_ROOT) {
  const [policy, supplements] = await Promise.all([
    loadTargetPolicy(repositoryRoot),
    loadSupplements(repositoryRoot),
  ]);
  const [rustComponents, pnpmComponents] = await Promise.all([
    collectRustComponents(repositoryRoot, policy, supplements),
    collectPnpmComponents(repositoryRoot, policy, supplements),
  ]);
  supplements.assertAllUsed();
  const components = mergeComponents([...rustComponents, ...pnpmComponents]);
  const inputs = await inputRecords(repositoryRoot, supplements.manifest);
  return {
    components,
    inputs,
    locks: inputs,
    policy,
    output: renderNotice({
      components,
      inputs,
      policy,
      repositoryRoot,
    }),
  };
}

function parseArguments(argv) {
  const options = {
    check: false,
    output: "THIRD_PARTY_LICENSES.txt",
    repositoryRoot: DEFAULT_REPOSITORY_ROOT,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--check") {
      options.check = true;
    } else if (argument === "--output") {
      options.output = argv[++index];
    } else if (argument === "--repo-root") {
      options.repositoryRoot = resolve(argv[++index]);
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  if (!options.output) throw new Error("--output requires a path");
  return options;
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const outputPath = resolve(options.repositoryRoot, options.output);
  const safeOutputPath = await resolveSafeNoticeOutputPath(
    options.repositoryRoot,
    outputPath,
  );
  const result = await generateNotice(options.repositoryRoot);
  if (options.check) {
    let existing;
    try {
      existing = await readFile(safeOutputPath, "utf8");
    } catch (error) {
      if (error.code === "ENOENT") {
        throw new Error(`${options.output} is missing; regenerate it`);
      }
      throw error;
    }
    if (existing !== result.output) {
      throw new Error(`${options.output} is stale; regenerate it`);
    }
  } else {
    await writeNoticeOutput(
      options.repositoryRoot,
      outputPath,
      result.output,
    );
  }
  const missingCount = result.components.filter(
    (component) => component.missingTextReason,
  ).length;
  if (options.check) assertDistributionReady(result.components);
  console.log(
    `${options.check ? "Verified" : "Generated"} ${options.output}: ` +
      `${result.components.length} components, ` +
      `${new Set(
        result.components.flatMap((component) =>
          component.documents.map((document) => document.hash),
        ),
      ).size} unique texts, ${missingCount} missing-text exception(s).`,
  );
}

if (resolve(process.argv[1] ?? "") === SCRIPT_PATH) {
  main().catch((error) => {
    console.error(`third-party notice generation failed: ${error.message}`);
    process.exitCode = 1;
  });
}
