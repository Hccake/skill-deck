import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const REQUIRED_TARGETS = Object.freeze([
  "darwin-aarch64",
  "darwin-x86_64",
  "linux-x86_64",
  "windows-x86_64",
]);

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function expectedArchitecture(platform) {
  return platform.slice(platform.lastIndexOf("-") + 1);
}

function validateAssetName(fragment) {
  invariant(
    path.basename(fragment.assetName) === fragment.assetName,
    "Asset filename must be a basename",
  );
  invariant(
    path.basename(fragment.signatureName) === fragment.signatureName,
    "Signature filename must be a basename",
  );
  invariant(
    fragment.signatureName === `${fragment.assetName}.sig`,
    "Signature filename must match the updater asset",
  );
  const [os, architecture] = fragment.platform.split(/-(?=[^-]+$)/);
  const expectedPrefix = `skill-deck_${fragment.version}_${os}_${architecture}.`;
  invariant(
    fragment.assetName.startsWith(expectedPrefix),
    `Stale or invalid asset filename: ${fragment.assetName}`,
  );
}

export function aggregateManifest({ fragments, tag, commit, repository }) {
  invariant(Array.isArray(fragments), "Fragments must be an array");
  invariant(
    /^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(tag),
    `Invalid tag: ${tag}`,
  );
  invariant(/^[0-9a-f]{40}$/.test(commit), `Invalid commit SHA: ${commit}`);
  invariant(
    /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository),
    `Invalid repository: ${repository}`,
  );
  const version = tag.slice(1);
  const byPlatform = new Map();

  for (const source of fragments) {
    const fragment = { ...source };
    invariant(
      !("url" in fragment),
      "Metadata fragments must not provide an updater URL",
    );
    invariant(
      REQUIRED_TARGETS.includes(fragment.platform),
      `Unexpected platform: ${fragment.platform}`,
    );
    invariant(
      !byPlatform.has(fragment.platform),
      `Duplicate platform: ${fragment.platform}`,
    );
    invariant(
      fragment.architecture === expectedArchitecture(fragment.platform),
      `Invalid architecture for ${fragment.platform}`,
    );
    invariant(
      fragment.version === version,
      `Fragment version does not match tag: ${fragment.version}`,
    );
    invariant(
      fragment.tag === tag,
      `Fragment tag does not match requested tag: ${fragment.tag}`,
    );
    invariant(
      fragment.commitSha === commit,
      `Fragment commit does not match requested commit: ${fragment.commitSha}`,
    );
    invariant(
      typeof fragment.signature === "string" && fragment.signature.trim(),
      `Missing signature for ${fragment.platform}`,
    );
    validateAssetName(fragment);
    byPlatform.set(fragment.platform, fragment);
  }

  const missing = REQUIRED_TARGETS.filter(
    (platform) => !byPlatform.has(platform),
  );
  invariant(
    missing.length === 0,
    `Missing required platform: ${missing.join(", ")}`,
  );

  const platforms = Object.fromEntries(
    REQUIRED_TARGETS.map((platform) => {
      const fragment = byPlatform.get(platform);
      const encodedAsset = fragment.assetName
        .split("/")
        .map(encodeURIComponent)
        .join("/");
      return [
        platform,
        {
          signature: fragment.signature,
          url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodedAsset}`,
        },
      ];
    }),
  );

  return { version, platforms };
}

function parseArgs(argv) {
  if (argv.includes("--help")) return { help: true };
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
  for (const key of ["input", "tag", "commit", "repository", "output"]) {
    invariant(args[key], `Missing --${key}`);
  }
  return args;
}

export async function readArtifactFragments(inputDirectory) {
  const entries = await fs.readdir(inputDirectory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const entryPath = path.join(inputDirectory, entry.name);
    if (entry.isDirectory()) {
      for (const child of await fs.readdir(entryPath, {
        withFileTypes: true,
      })) {
        if (child.isFile() && child.name === "metadata.json")
          files.push(path.join(entryPath, child.name));
      }
    } else if (entry.isFile() && entry.name === "metadata.json") {
      files.push(entryPath);
    }
  }
  files.sort();
  return Promise.all(files.map(validateFragmentFiles));
}

async function validateFragmentFiles(metadataFile) {
  const fragment = JSON.parse(await fs.readFile(metadataFile, "utf8"));
  validateAssetName(fragment);
  const directory = path.dirname(metadataFile);
  const assetPath = path.join(directory, fragment.assetName);
  const signaturePath = path.join(directory, fragment.signatureName);
  const [asset, signature] = await Promise.all([
    fs.lstat(assetPath).catch(() => null),
    fs.lstat(signaturePath).catch(() => null),
  ]);
  invariant(
    asset?.isFile() && !asset.isSymbolicLink() && asset.size > 0,
    `Missing or empty updater asset: ${fragment.assetName}`,
  );
  invariant(
    signature?.isFile() && !signature.isSymbolicLink() && signature.size > 0,
    `Missing or empty updater signature: ${fragment.signatureName}`,
  );
  const signatureText = (await fs.readFile(signaturePath, "utf8")).trim();
  invariant(
    signatureText === fragment.signature.trim(),
    `Updater signature content mismatch: ${fragment.signatureName}`,
  );
  return fragment;
}

async function writeAtomically(output, value) {
  const directory = path.dirname(output);
  await fs.mkdir(directory, { recursive: true });
  const temporary = `${output}.tmp-${process.pid}`;
  await fs.writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, {
    flag: "wx",
  });
  try {
    await fs.rename(temporary, output);
  } catch (error) {
    await fs.rm(temporary, { force: true });
    throw error;
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(
      "Usage: node scripts/aggregate-updater-manifest.mjs --input DIR --tag TAG --commit SHA --repository OWNER/REPO --output FILE",
    );
    return;
  }
  const fragments = await readArtifactFragments(args.input);
  const manifest = aggregateManifest({
    fragments,
    tag: args.tag,
    commit: args.commit,
    repository: args.repository,
  });
  await writeAtomically(args.output, manifest);
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
