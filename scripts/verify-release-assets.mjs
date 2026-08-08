import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PLATFORM_ASSET_TEMPLATES = Object.freeze({
  "darwin-aarch64": "skill-deck_{version}_macos_aarch64.app.tar.gz",
  "darwin-aarch64-app": "skill-deck_{version}_macos_aarch64.app.tar.gz",
  "darwin-x86_64": "skill-deck_{version}_macos_x64.app.tar.gz",
  "darwin-x86_64-app": "skill-deck_{version}_macos_x64.app.tar.gz",
  "linux-x86_64": "skill-deck_{version}_linux_amd64.AppImage",
  "linux-x86_64-appimage": "skill-deck_{version}_linux_amd64.AppImage",
  "linux-x86_64-deb": "skill-deck_{version}_linux_amd64.deb",
  "linux-x86_64-rpm": "skill-deck_{version}_linux_x86_64.rpm",
  "windows-x86_64": "skill-deck_{version}_windows_x64.msi",
  "windows-x86_64-msi": "skill-deck_{version}_windows_x64.msi",
  "windows-x86_64-nsis": "skill-deck_{version}_windows_x64-setup.exe",
});

const INSTALLER_TEMPLATES = Object.freeze([
  "skill-deck_{version}_macos_aarch64.dmg",
  "skill-deck_{version}_macos_x64.dmg",
  "skill-deck_{version}_linux_amd64.AppImage",
  "skill-deck_{version}_linux_amd64.deb",
  "skill-deck_{version}_linux_x86_64.rpm",
  "skill-deck_{version}_windows_x64-setup.exe",
  "skill-deck_{version}_windows_x64.msi",
]);

const RELEASE_ASSET_TEMPLATES = Object.freeze([
  "latest.json",
  "skill-deck_{version}_linux_amd64.AppImage",
  "skill-deck_{version}_linux_amd64.AppImage.sig",
  "skill-deck_{version}_linux_amd64.deb",
  "skill-deck_{version}_linux_amd64.deb.sig",
  "skill-deck_{version}_linux_x86_64.rpm",
  "skill-deck_{version}_linux_x86_64.rpm.sig",
  "skill-deck_{version}_macos_aarch64.app.tar.gz",
  "skill-deck_{version}_macos_aarch64.app.tar.gz.sig",
  "skill-deck_{version}_macos_aarch64.dmg",
  "skill-deck_{version}_macos_x64.app.tar.gz",
  "skill-deck_{version}_macos_x64.app.tar.gz.sig",
  "skill-deck_{version}_macos_x64.dmg",
  "skill-deck_{version}_windows_x64-setup.exe",
  "skill-deck_{version}_windows_x64-setup.exe.sig",
  "skill-deck_{version}_windows_x64.msi",
  "skill-deck_{version}_windows_x64.msi.sig",
]);

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function render(template, version) {
  return template.replace("{version}", version);
}

function normalizeText(value) {
  return value.replaceAll("\r\n", "\n").trimEnd();
}

function validateVersion(version) {
  invariant(
    typeof version === "string" &&
      /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version),
    `Invalid release version: ${version}`,
  );
}

export function documentedInstallerNames(version) {
  invariant(typeof version === "string" && version, "Missing release version");
  return INSTALLER_TEMPLATES.map((template) => render(template, version));
}

export function expectedReleaseAssetNames(version) {
  validateVersion(version);
  return RELEASE_ASSET_TEMPLATES.map((template) => render(template, version));
}

function verifyDraftState({ release, tag, expectedPrerelease }) {
  invariant(release && typeof release === "object", "Invalid Release state");
  invariant(release.isDraft === true, "Refusing to modify a published Release");
  invariant(
    release.tagName === tag,
    `Release tag does not match requested tag: ${release.tagName}`,
  );
  invariant(
    typeof expectedPrerelease === "boolean",
    "Release verification requires expected prerelease state",
  );
  invariant(
    release.isPrerelease === expectedPrerelease,
    "Release prerelease state does not match version",
  );
}

function verifyAssetApiUrl(apiUrl, repository, name) {
  invariant(typeof apiUrl === "string", `Release asset has no API URL: ${name}`);
  const match = apiUrl.match(
    /^https:\/\/api\.github\.com\/repos\/([^/]+\/[^/]+)\/releases\/assets\/(\d+)$/i,
  );
  invariant(match, `Invalid Release asset API URL: ${apiUrl}`);
  invariant(
    match[1].toLowerCase() === repository.toLowerCase(),
    `Release asset belongs to another repository: ${apiUrl}`,
  );
}

function readRemoteAssets(release, expectedNames, repository) {
  invariant(Array.isArray(release.assets), "Release assets must be an array");
  invariant(
    release.assets.length === expectedNames.length,
    `Expected 17 Release assets, found ${release.assets.length}`,
  );

  const assets = new Map();
  for (const asset of release.assets) {
    invariant(
      asset && typeof asset.name === "string" && Number.isSafeInteger(asset.size),
      "Invalid remote Release asset",
    );
    invariant(asset.size > 0, `Missing or empty Release asset: ${asset.name}`);
    invariant(!assets.has(asset.name), `Duplicate Release asset: ${asset.name}`);
    verifyAssetApiUrl(asset.apiUrl, repository, asset.name);
    assets.set(asset.name, asset);
  }
  for (const name of expectedNames) {
    invariant(assets.has(name), `Missing expected Release asset: ${name}`);
  }
  return assets;
}

function expectedPlatformAssets(version) {
  return Object.fromEntries(
    Object.entries(PLATFORM_ASSET_TEMPLATES).map(([platform, template]) => [
      platform,
      render(template, version),
    ]),
  );
}

async function readDownloadedAsset(directory, name, remoteAssets) {
  const file = path.join(directory, name);
  const stat = await fs.lstat(file).catch(() => null);
  invariant(
    stat?.isFile() && !stat.isSymbolicLink() && stat.size > 0,
    `Missing or empty downloaded Release asset: ${name}`,
  );
  invariant(
    stat.size === remoteAssets.get(name)?.size,
    `Downloaded Release asset size does not match GitHub: ${name}`,
  );
  return fs.readFile(file, "utf8");
}

async function verifyCompleteRelease({
  release,
  manifest,
  signaturesDirectory,
  notes,
  version,
  tag,
  repository,
}) {
  invariant(tag === `v${version}`, `Release tag does not match version: ${tag}`);
  invariant(
    /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository),
    `Invalid repository: ${repository}`,
  );
  invariant(typeof release.body === "string", "Release body must be a string");
  invariant(
    normalizeText(release.body) === normalizeText(notes),
    "Release notes do not match CHANGELOG.md",
  );

  const expectedNames = expectedReleaseAssetNames(version);
  const remoteAssets = readRemoteAssets(release, expectedNames, repository);
  await readDownloadedAsset(signaturesDirectory, "latest.json", remoteAssets);

  invariant(manifest && typeof manifest === "object", "Invalid latest.json");
  invariant(
    manifest.version === version,
    `Updater version does not match release: ${manifest.version}`,
  );
  invariant(
    typeof manifest.notes === "string" &&
      normalizeText(manifest.notes) === normalizeText(notes),
    "Updater notes do not match Release notes",
  );
  invariant(
    typeof manifest.pub_date === "string" &&
      !Number.isNaN(Date.parse(manifest.pub_date)),
    "Updater publication date is invalid",
  );
  invariant(
    manifest.platforms && typeof manifest.platforms === "object",
    "Updater platforms must be an object",
  );

  const platformAssets = expectedPlatformAssets(version);
  const platformKeys = Object.keys(manifest.platforms).sort();
  const expectedKeys = Object.keys(platformAssets).sort();
  invariant(
    platformKeys.length === expectedKeys.length,
    `Expected 11 updater platform keys, found ${platformKeys.length}`,
  );
  invariant(
    platformKeys.every((key, index) => key === expectedKeys[index]),
    `Updater platform keys do not match: ${platformKeys.join(", ")}`,
  );

  const signatures = new Map();
  for (const assetName of new Set(Object.values(platformAssets))) {
    const signatureName = `${assetName}.sig`;
    const signature = await readDownloadedAsset(
      signaturesDirectory,
      signatureName,
      remoteAssets,
    );
    signatures.set(assetName, signature.trim());
  }

  for (const [platform, assetName] of Object.entries(platformAssets)) {
    const entry = manifest.platforms[platform];
    invariant(entry && typeof entry === "object", `Missing updater platform: ${platform}`);
    invariant(
      entry.url === remoteAssets.get(assetName)?.apiUrl,
      `Updater URL does not match ${platform}: ${entry.url}`,
    );
    invariant(
      typeof entry.signature === "string" &&
        entry.signature.trim() === signatures.get(assetName),
      `Updater signature does not match ${platform}`,
    );
  }
}

export async function verifyReleaseAssets({
  mode,
  release,
  manifest,
  signaturesDirectory,
  notes,
  version,
  tag,
  repository,
  expectedPrerelease,
}) {
  invariant(
    mode === "draft" || mode === "complete",
    `Invalid verification mode: ${mode}`,
  );
  invariant(typeof tag === "string" && tag, "Missing release tag");
  verifyDraftState({ release, tag, expectedPrerelease });

  if (mode === "draft") return;

  validateVersion(version);
  invariant(
    typeof signaturesDirectory === "string" && signaturesDirectory,
    "Complete verification requires downloaded signatures",
  );
  invariant(typeof notes === "string", "Complete verification requires Release notes");
  await verifyCompleteRelease({
    release,
    manifest,
    signaturesDirectory,
    notes,
    version,
    tag,
    repository,
  });
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
  for (const key of ["mode", "release", "tag", "expected-prerelease"]) {
    invariant(args[key], `Missing --${key}`);
  }
  if (args.mode === "complete") {
    for (const key of [
      "manifest",
      "signatures",
      "notes",
      "version",
      "repository",
    ]) {
      invariant(args[key], `Missing --${key}`);
    }
  }
  return args;
}

function parseBoolean(value) {
  invariant(value === "true" || value === "false", `Invalid boolean: ${value}`);
  return value === "true";
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const release = JSON.parse(await fs.readFile(args.release, "utf8"));
  const manifest = args.manifest
    ? JSON.parse(await fs.readFile(args.manifest, "utf8"))
    : undefined;
  const notes = args.notes ? await fs.readFile(args.notes, "utf8") : undefined;
  await verifyReleaseAssets({
    mode: args.mode,
    release,
    manifest,
    signaturesDirectory: args.signatures,
    notes,
    version: args.version,
    tag: args.tag,
    repository: args.repository,
    expectedPrerelease: parseBoolean(args["expected-prerelease"]),
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
