import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { verifyReleaseAssets } from "../verify-release-assets.mjs";

const verifierPath = fileURLToPath(
  new URL("../verify-release-assets.mjs", import.meta.url),
);

async function createLocalArtifacts() {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-release-assets-"));
  const inputDirectory = path.join(root, "release-artifacts");
  const artifacts = [
    [
      "updater-darwin-aarch64",
      "skill-deck_1.7.0-beta.1_darwin_aarch64.tar.gz",
      "mac",
    ],
    [
      "updater-windows-x86_64",
      "skill-deck_1.7.0-beta.1_windows_x86_64.exe",
      "windows",
    ],
  ];

  for (const [directory, name, contents] of artifacts) {
    const artifactDirectory = path.join(inputDirectory, directory);
    await mkdir(artifactDirectory, { recursive: true });
    await writeFile(path.join(artifactDirectory, name), contents);
    await writeFile(
      path.join(artifactDirectory, `${name}.sig`),
      `signature-${contents}`,
    );
    await writeFile(path.join(artifactDirectory, "metadata.json"), "{}");
  }

  return { root, inputDirectory, artifacts };
}

function remoteAssets(artifacts) {
  return artifacts.flatMap(([, name, contents]) => [
    { name, size: Buffer.byteLength(contents) },
    { name: `${name}.sig`, size: Buffer.byteLength(`signature-${contents}`) },
  ]);
}

test("accepts a matching prerelease draft before mutation", async () => {
  const { root, inputDirectory, artifacts } = await createLocalArtifacts();
  try {
    await verifyReleaseAssets({
      mode: "draft",
      release: {
        isDraft: true,
        isPrerelease: true,
        assets: [
          ...remoteAssets(artifacts),
          { name: "release-notes.txt", size: 5 },
        ],
      },
      inputDirectory,
      expectedPrerelease: true,
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

for (const [name, mutate, expected] of [
  [
    "published Release",
    (release) => {
      release.isDraft = false;
    },
    /published Release/i,
  ],
  [
    "prerelease mismatch",
    (release) => {
      release.isPrerelease = false;
    },
    /prerelease state/i,
  ],
  [
    "stale managed asset",
    (release) => {
      release.assets.push({
        name: "skill-deck_1.6.2_windows_x86_64.exe",
        size: 10,
      });
    },
    /unexpected managed updater asset/i,
  ],
  [
    "internal metadata",
    (release) => {
      release.assets.push({ name: "metadata.json", size: 10 });
    },
    /metadata fragments/i,
  ],
]) {
  test(`rejects ${name} before mutation`, async () => {
    const { root, inputDirectory, artifacts } = await createLocalArtifacts();
    try {
      const release = {
        isDraft: true,
        isPrerelease: true,
        assets: remoteAssets(artifacts),
      };
      mutate(release);
      await assert.rejects(
        () =>
          verifyReleaseAssets({
            mode: "draft",
            release,
            inputDirectory,
            expectedPrerelease: true,
          }),
        expected,
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
}

test("accepts uploaded assets when every local file has the same remote size", async () => {
  const { root, inputDirectory, artifacts } = await createLocalArtifacts();
  try {
    await verifyReleaseAssets({
      mode: "uploaded",
      release: { assets: remoteAssets(artifacts) },
      inputDirectory,
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("supports the CLI contract used by the Release workflow", async () => {
  const { root, inputDirectory, artifacts } = await createLocalArtifacts();
  const releaseFile = path.join(root, "release-state.json");
  try {
    await writeFile(
      releaseFile,
      JSON.stringify({
        isDraft: true,
        isPrerelease: true,
        assets: remoteAssets(artifacts),
      }),
    );

    execFileSync(process.execPath, [
      verifierPath,
      "--mode",
      "draft",
      "--release",
      releaseFile,
      "--input",
      inputDirectory,
      "--expected-prerelease",
      "true",
    ]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

for (const [name, mutate] of [
  ["missing remote asset", (assets) => assets.pop()],
  [
    "remote size mismatch",
    (assets) => {
      assets[0].size += 1;
    },
  ],
]) {
  test(`rejects ${name} after upload`, async () => {
    const { root, inputDirectory, artifacts } = await createLocalArtifacts();
    try {
      const assets = remoteAssets(artifacts);
      mutate(assets);
      await assert.rejects(
        () =>
          verifyReleaseAssets({
            mode: "uploaded",
            release: { assets },
            inputDirectory,
          }),
        /remote asset mismatch/i,
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
}
