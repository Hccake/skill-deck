import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  documentedInstallerNames,
  expectedReleaseAssetNames,
  verifyReleaseAssets,
} from "../verify-release-assets.mjs";

const verifierPath = fileURLToPath(
  new URL("../verify-release-assets.mjs", import.meta.url),
);
const version = "1.7.0-beta.4";
const tag = `v${version}`;
const repository = "hccake/skill-deck";
const notes = "## Changes\n\n- Complete updater matrix.\n";

const platformAssets = Object.freeze({
  "darwin-aarch64": `skill-deck_${version}_macos_aarch64.app.tar.gz`,
  "darwin-aarch64-app": `skill-deck_${version}_macos_aarch64.app.tar.gz`,
  "darwin-x86_64": `skill-deck_${version}_macos_x64.app.tar.gz`,
  "darwin-x86_64-app": `skill-deck_${version}_macos_x64.app.tar.gz`,
  "linux-x86_64": `skill-deck_${version}_linux_amd64.AppImage`,
  "linux-x86_64-appimage": `skill-deck_${version}_linux_amd64.AppImage`,
  "linux-x86_64-deb": `skill-deck_${version}_linux_amd64.deb`,
  "linux-x86_64-rpm": `skill-deck_${version}_linux_x86_64.rpm`,
  "windows-x86_64": `skill-deck_${version}_windows_x64.msi`,
  "windows-x86_64-msi": `skill-deck_${version}_windows_x64.msi`,
  "windows-x86_64-nsis": `skill-deck_${version}_windows_x64-setup.exe`,
});

async function createCompleteFixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-release-"));
  const signaturesDirectory = path.join(root, "release-downloads");
  await mkdir(signaturesDirectory);

  const signatureValues = new Map();
  const downloadedSizes = new Map();
  for (const assetName of new Set(Object.values(platformAssets))) {
    const signature = `signature:${assetName}`;
    const signatureContents = `${signature}\n`;
    signatureValues.set(assetName, signature);
    downloadedSizes.set(`${assetName}.sig`, Buffer.byteLength(signatureContents));
    await writeFile(
      path.join(signaturesDirectory, `${assetName}.sig`),
      signatureContents,
    );
  }

  const releaseAssetNames = expectedReleaseAssetNames(version);
  const assetApiUrls = new Map(
    releaseAssetNames.map((name, index) => [
      name,
      `https://api.github.com/repos/hccake/skill-deck/releases/assets/${1000 + index}`,
    ]),
  );
  const manifest = {
    version,
    notes,
    pub_date: "2026-08-08T12:00:00.000Z",
    platforms: Object.fromEntries(
      Object.entries(platformAssets).map(([platform, assetName]) => [
        platform,
        {
          signature: signatureValues.get(assetName),
          url: assetApiUrls.get(assetName),
        },
      ]),
    ),
  };
  const manifestFile = path.join(signaturesDirectory, "latest.json");
  const notesFile = path.join(root, "release-notes.md");
  const manifestContents = `${JSON.stringify(manifest, null, 2)}\n`;
  downloadedSizes.set("latest.json", Buffer.byteLength(manifestContents));
  await writeFile(manifestFile, manifestContents);
  await writeFile(notesFile, notes);

  const release = {
    tagName: tag,
    isDraft: true,
    isPrerelease: true,
    body: notes.trimEnd(),
    assets: releaseAssetNames.map((name) => ({
      name,
      size: downloadedSizes.get(name) ?? 100,
      apiUrl: assetApiUrls.get(name),
    })),
  };

  return {
    root,
    signaturesDirectory,
    manifestFile,
    notesFile,
    manifest,
    release,
  };
}

test("declares the 17 uploaded assets and seven documented installers", () => {
  assert.deepEqual(expectedReleaseAssetNames(version), [
    "latest.json",
    `skill-deck_${version}_linux_amd64.AppImage`,
    `skill-deck_${version}_linux_amd64.AppImage.sig`,
    `skill-deck_${version}_linux_amd64.deb`,
    `skill-deck_${version}_linux_amd64.deb.sig`,
    `skill-deck_${version}_linux_x86_64.rpm`,
    `skill-deck_${version}_linux_x86_64.rpm.sig`,
    `skill-deck_${version}_macos_aarch64.app.tar.gz`,
    `skill-deck_${version}_macos_aarch64.app.tar.gz.sig`,
    `skill-deck_${version}_macos_aarch64.dmg`,
    `skill-deck_${version}_macos_x64.app.tar.gz`,
    `skill-deck_${version}_macos_x64.app.tar.gz.sig`,
    `skill-deck_${version}_macos_x64.dmg`,
    `skill-deck_${version}_windows_x64-setup.exe`,
    `skill-deck_${version}_windows_x64-setup.exe.sig`,
    `skill-deck_${version}_windows_x64.msi`,
    `skill-deck_${version}_windows_x64.msi.sig`,
  ]);
  assert.deepEqual(documentedInstallerNames(version), [
    `skill-deck_${version}_macos_aarch64.dmg`,
    `skill-deck_${version}_macos_x64.dmg`,
    `skill-deck_${version}_linux_amd64.AppImage`,
    `skill-deck_${version}_linux_amd64.deb`,
    `skill-deck_${version}_linux_x86_64.rpm`,
    `skill-deck_${version}_windows_x64-setup.exe`,
    `skill-deck_${version}_windows_x64.msi`,
  ]);
});

test("accepts a matching prerelease draft before platform uploads", async () => {
  await verifyReleaseAssets({
    mode: "draft",
    release: {
      tagName: tag,
      isDraft: true,
      isPrerelease: true,
      assets: [],
    },
    tag,
    expectedPrerelease: true,
  });
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
    "tag mismatch",
    (release) => {
      release.tagName = "v1.7.0-beta.3";
    },
    /tag does not match/i,
  ],
]) {
  test(`rejects ${name} before mutation`, async () => {
    const release = {
      tagName: tag,
      isDraft: true,
      isPrerelease: true,
      assets: [],
    };
    mutate(release);
    await assert.rejects(
      () =>
        verifyReleaseAssets({
          mode: "draft",
          release,
          tag,
          expectedPrerelease: true,
        }),
      expected,
    );
  });
}

test("accepts the complete official tauri-action release contract", async () => {
  const fixture = await createCompleteFixture();
  try {
    await verifyReleaseAssets({
      mode: "complete",
      release: fixture.release,
      manifest: fixture.manifest,
      signaturesDirectory: fixture.signaturesDirectory,
      notes,
      version,
      tag,
      repository,
      expectedPrerelease: true,
    });
  } finally {
    await rm(fixture.root, { recursive: true, force: true });
  }
});

for (const [name, mutate, expected] of [
  [
    "missing Release asset",
    ({ release }) => release.assets.pop(),
    /Expected 17 Release assets/i,
  ],
  [
    "empty Release asset",
    ({ release }) => {
      release.assets[0].size = 0;
    },
    /empty Release asset/i,
  ],
  [
    "missing updater platform",
    ({ manifest }) => delete manifest.platforms["linux-x86_64-deb"],
    /Expected 11 updater platform keys/i,
  ],
  [
    "wrong updater URL",
    ({ manifest }) => {
      manifest.platforms["windows-x86_64-nsis"].url =
        "https://example.com/installer.exe";
    },
    /Updater URL does not match/i,
  ],
  [
    "wrong updater signature",
    ({ manifest }) => {
      manifest.platforms["linux-x86_64-rpm"].signature = "wrong";
    },
    /Updater signature does not match/i,
  ],
  [
    "manifest notes mismatch",
    ({ manifest }) => {
      manifest.notes = "different";
    },
    /updater notes do not match/i,
  ],
]) {
  test(`rejects ${name} after upload`, async () => {
    const fixture = await createCompleteFixture();
    try {
      mutate(fixture);
      await assert.rejects(
        () =>
          verifyReleaseAssets({
            mode: "complete",
            release: fixture.release,
            manifest: fixture.manifest,
            signaturesDirectory: fixture.signaturesDirectory,
            notes,
            version,
            tag,
            repository,
            expectedPrerelease: true,
          }),
        expected,
      );
    } finally {
      await rm(fixture.root, { recursive: true, force: true });
    }
  });
}

test("supports the complete CLI contract used by the Release workflow", async () => {
  const fixture = await createCompleteFixture();
  const releaseFile = path.join(fixture.root, "release-state.json");
  try {
    await writeFile(releaseFile, JSON.stringify(fixture.release));
    execFileSync(process.execPath, [
      verifierPath,
      "--mode",
      "complete",
      "--release",
      releaseFile,
      "--manifest",
      fixture.manifestFile,
      "--signatures",
      fixture.signaturesDirectory,
      "--notes",
      fixture.notesFile,
      "--version",
      version,
      "--tag",
      tag,
      "--repository",
      repository,
      "--expected-prerelease",
      "true",
    ]);
  } finally {
    await rm(fixture.root, { recursive: true, force: true });
  }
});
