import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  UPDATER_ARTIFACT_CONTRACTS,
  packageUpdaterArtifact,
  selectUpdaterArtifact,
} from "../package-updater-artifact.mjs";

const commitSha = "0123456789abcdef0123456789abcdef01234567";

test("declares the Tauri v2 updater artifact contract for every release platform", () => {
  assert.deepEqual(UPDATER_ARTIFACT_CONTRACTS, {
    "darwin-aarch64": { sourceSuffix: ".app.tar.gz", assetExtension: "tar.gz" },
    "darwin-x86_64": { sourceSuffix: ".app.tar.gz", assetExtension: "tar.gz" },
    "linux-x86_64": { sourceSuffix: ".AppImage", assetExtension: "AppImage" },
    "windows-x86_64": { sourceSuffix: "-setup.exe", assetExtension: "exe" },
  });
});

test("selects exactly one matching updater artifact from tauri-action output", () => {
  const source = selectUpdaterArtifact({
    platform: "windows-x86_64",
    artifactPaths: [
      "D:/work/target/release/bundle/nsis/skill-deck_1.7.0_x64-setup.exe",
      "D:/work/target/release/bundle/nsis/skill-deck_1.7.0_x64-setup.exe.sig",
    ],
  });

  assert.equal(source, "D:/work/target/release/bundle/nsis/skill-deck_1.7.0_x64-setup.exe");
});

for (const [name, artifactPaths, expected] of [
  ["missing artifact", [], /expected one updater artifact/i],
  [
    "legacy v1-compatible archive",
    ["/tmp/skill-deck_1.7.0_amd64.AppImage.tar.gz"],
    /expected one updater artifact/i,
  ],
  [
    "multiple matching artifacts",
    ["/tmp/one.AppImage", "/tmp/two.AppImage"],
    /expected one updater artifact/i,
  ],
]) {
  test(`rejects ${name}`, () => {
    assert.throws(
      () => selectUpdaterArtifact({ platform: "linux-x86_64", artifactPaths }),
      expected,
    );
  });
}

test("copies a signed updater package into canonical release files and metadata", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-updater-package-"));
  try {
    const sourceDirectory = path.join(root, "bundle");
    const outputDirectory = path.join(root, "release-artifact");
    const source = path.join(sourceDirectory, "Skill Deck.app.tar.gz");
    await mkdir(sourceDirectory, { recursive: true });
    await writeFile(source, "updater package");
    await writeFile(`${source}.sig`, "updater signature\n");

    const metadata = await packageUpdaterArtifact({
      platform: "darwin-aarch64",
      version: "1.7.0-beta.1",
      tag: "v1.7.0-beta.1",
      commitSha,
      artifactPaths: [source, `${source}.sig`],
      outputDirectory,
    });

    assert.deepEqual(metadata, {
      platform: "darwin-aarch64",
      architecture: "aarch64",
      version: "1.7.0-beta.1",
      tag: "v1.7.0-beta.1",
      commitSha,
      assetName: "skill-deck_1.7.0-beta.1_darwin_aarch64.tar.gz",
      signatureName: "skill-deck_1.7.0-beta.1_darwin_aarch64.tar.gz.sig",
      signature: "updater signature",
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("rejects a missing or empty updater signature", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-updater-package-"));
  try {
    const source = path.join(root, "skill-deck.AppImage");
    await writeFile(source, "updater package");

    await assert.rejects(
      () =>
        packageUpdaterArtifact({
          platform: "linux-x86_64",
          version: "1.7.0-beta.1",
          tag: "v1.7.0-beta.1",
          commitSha,
          artifactPaths: [source],
          outputDirectory: path.join(root, "release-artifact"),
        }),
      /signature/i,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
