import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

async function readLocalAssets(inputDirectory) {
  const assets = [];

  async function visit(directory) {
    const entries = await fs.readdir(directory, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        await visit(entryPath);
      } else if (entry.isFile() && entry.name !== "metadata.json") {
        const stat = await fs.lstat(entryPath);
        invariant(stat.size > 0, `Missing or empty local asset: ${entry.name}`);
        assets.push({ name: entry.name, size: stat.size });
      } else if (!entry.isFile()) {
        throw new Error(`Unsupported release artifact entry: ${entryPath}`);
      }
    }
  }

  await visit(inputDirectory);
  assets.sort((left, right) => left.name.localeCompare(right.name));

  const names = new Set();
  for (const asset of assets) {
    invariant(
      !names.has(asset.name),
      `Duplicate local asset name: ${asset.name}`,
    );
    names.add(asset.name);
  }
  invariant(assets.length > 0, "No local release assets found");
  return assets;
}

function readRemoteAssets(release) {
  invariant(release && typeof release === "object", "Invalid Release state");
  invariant(Array.isArray(release.assets), "Release assets must be an array");

  const assets = new Map();
  for (const asset of release.assets) {
    invariant(
      asset && typeof asset.name === "string" && Number.isSafeInteger(asset.size),
      "Invalid remote Release asset",
    );
    invariant(!assets.has(asset.name), `Duplicate remote asset: ${asset.name}`);
    assets.set(asset.name, asset.size);
  }
  return assets;
}

function verifyManagedAssets(remoteAssets, expectedNames) {
  for (const name of remoteAssets.keys()) {
    if (name === "metadata.json" || name.endsWith("/metadata.json")) {
      throw new Error("Metadata fragments must never be published");
    }
    if (name.startsWith("skill-deck_") && !expectedNames.has(name)) {
      throw new Error(`Unexpected managed updater asset: ${name}`);
    }
  }
}

export async function verifyReleaseAssets({
  mode,
  release,
  inputDirectory,
  expectedPrerelease,
}) {
  invariant(
    mode === "draft" || mode === "uploaded",
    `Invalid verification mode: ${mode}`,
  );

  const remoteAssets = readRemoteAssets(release);
  if (mode === "draft") {
    invariant(
      release.isDraft === true,
      "Refusing to modify a published Release",
    );
    invariant(
      typeof expectedPrerelease === "boolean",
      "Draft verification requires expected prerelease state",
    );
    invariant(
      release.isPrerelease === expectedPrerelease,
      "Release prerelease state does not match version",
    );
  }

  const localAssets = await readLocalAssets(inputDirectory);
  const expectedNames = new Set(localAssets.map((asset) => asset.name));
  verifyManagedAssets(remoteAssets, expectedNames);

  if (mode === "draft") {
    return;
  }

  for (const asset of localAssets) {
    invariant(
      remoteAssets.get(asset.name) === asset.size,
      `Remote asset mismatch: ${asset.name}`,
    );
  }
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    invariant(
      key?.startsWith("--") && value,
      `Invalid CLI argument near ${key ?? "<end>"}`,
    );
    args[key.slice(2)] = value;
  }
  for (const key of ["mode", "release", "input"]) {
    invariant(args[key], `Missing --${key}`);
  }
  return args;
}

function parseBoolean(value) {
  invariant(
    value === "true" || value === "false",
    `Invalid boolean: ${value}`,
  );
  return value === "true";
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const release = JSON.parse(await fs.readFile(args.release, "utf8"));
  await verifyReleaseAssets({
    mode: args.mode,
    release,
    inputDirectory: args.input,
    expectedPrerelease:
      args["expected-prerelease"] === undefined
        ? undefined
        : parseBoolean(args["expected-prerelease"]),
  });
}

if (
  process.argv[1] &&
  fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
