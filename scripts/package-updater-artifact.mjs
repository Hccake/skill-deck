import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const UPDATER_ARTIFACT_CONTRACTS = Object.freeze({
  "darwin-aarch64": Object.freeze({
    sourceSuffix: ".app.tar.gz",
    assetExtension: "tar.gz",
  }),
  "darwin-x86_64": Object.freeze({
    sourceSuffix: ".app.tar.gz",
    assetExtension: "tar.gz",
  }),
  "linux-x86_64": Object.freeze({
    sourceSuffix: ".AppImage",
    assetExtension: "AppImage",
  }),
  "windows-x86_64": Object.freeze({
    sourceSuffix: "-setup.exe",
    assetExtension: "exe",
  }),
});

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function artifactContract(platform) {
  const contract = UPDATER_ARTIFACT_CONTRACTS[platform];
  invariant(contract, `Unsupported updater platform: ${platform}`);
  return contract;
}

function platformParts(platform) {
  const separator = platform.lastIndexOf("-");
  invariant(separator > 0, `Invalid updater platform: ${platform}`);
  return {
    os: platform.slice(0, separator),
    architecture: platform.slice(separator + 1),
  };
}

export function selectUpdaterArtifact({ platform, artifactPaths }) {
  const { sourceSuffix } = artifactContract(platform);
  invariant(Array.isArray(artifactPaths), "Tauri artifact paths must be an array");
  const candidates = artifactPaths.filter(
    (artifactPath) =>
      typeof artifactPath === "string" &&
      artifactPath.endsWith(sourceSuffix) &&
      !artifactPath.endsWith(".sig"),
  );
  invariant(
    candidates.length === 1,
    `Expected one updater artifact for ${platform}, found ${candidates.length}`,
  );
  return candidates[0];
}

async function readNonEmptyFile(file, description) {
  const stat = await fs.lstat(file).catch(() => null);
  invariant(
    stat?.isFile() && !stat.isSymbolicLink() && stat.size > 0,
    `Missing or empty ${description}: ${file}`,
  );
  return fs.readFile(file);
}

export async function packageUpdaterArtifact({
  platform,
  version,
  tag,
  commitSha,
  artifactPaths,
  outputDirectory,
}) {
  invariant(typeof version === "string" && version, "Missing release version");
  invariant(typeof tag === "string" && tag, "Missing release tag");
  invariant(/^[0-9a-f]{40}$/.test(commitSha), "Invalid release commit SHA");
  invariant(typeof outputDirectory === "string" && outputDirectory, "Missing output directory");

  const source = selectUpdaterArtifact({ platform, artifactPaths });
  const sourceSignature = `${source}.sig`;
  const [, signatureBuffer] = await Promise.all([
    readNonEmptyFile(source, "updater artifact"),
    readNonEmptyFile(sourceSignature, "updater signature"),
  ]);
  const signature = signatureBuffer.toString("utf8").trim();
  invariant(signature, `Empty updater signature: ${sourceSignature}`);

  const { assetExtension } = artifactContract(platform);
  const { os, architecture } = platformParts(platform);
  const assetName = `skill-deck_${version}_${os}_${architecture}.${assetExtension}`;
  const signatureName = `${assetName}.sig`;
  const metadata = {
    platform,
    architecture,
    version,
    tag,
    commitSha,
    assetName,
    signatureName,
    signature,
  };

  await fs.mkdir(outputDirectory, { recursive: true });
  await Promise.all([
    fs.copyFile(source, path.join(outputDirectory, assetName)),
    fs.copyFile(sourceSignature, path.join(outputDirectory, signatureName)),
  ]);
  await fs.writeFile(
    path.join(outputDirectory, "metadata.json"),
    `${JSON.stringify(metadata, null, 2)}\n`,
  );
  return metadata;
}

function artifactPathsFromEnvironment() {
  try {
    return JSON.parse(process.env.TAURI_ARTIFACT_PATHS ?? "");
  } catch {
    throw new Error("TAURI_ARTIFACT_PATHS must be a JSON array from tauri-action");
  }
}

async function main() {
  await packageUpdaterArtifact({
    platform: process.env.PLATFORM_KEY,
    version: process.env.VERSION,
    tag: process.env.TAG,
    commitSha: process.env.COMMIT_SHA,
    artifactPaths: artifactPathsFromEnvironment(),
    outputDirectory: process.env.OUTPUT_DIRECTORY ?? "release-artifact",
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
