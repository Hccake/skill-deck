import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  REQUIRED_TARGETS,
  aggregateManifest,
  readArtifactFragments,
} from "../aggregate-updater-manifest.mjs";

const commitSha = "0123456789abcdef0123456789abcdef01234567";

function fragment(platform, architecture, extension) {
  const assetName = `skill-deck_1.6.2_${platform}_${architecture}.${extension}`;
  return {
    platform: `${platform}-${architecture}`,
    architecture,
    version: "1.6.2",
    tag: "v1.6.2",
    commitSha,
    assetName,
    signatureName: `${assetName}.sig`,
    signature: `signature-${platform}-${architecture}`,
  };
}

function completeFragments() {
  return [
    fragment("darwin", "aarch64", "tar.gz"),
    fragment("darwin", "x86_64", "tar.gz"),
    fragment("linux", "x86_64", "AppImage.tar.gz"),
    fragment("windows", "x86_64", "nsis.zip"),
  ];
}

test("exports the version-controlled updater target matrix", () => {
  assert.deepEqual(REQUIRED_TARGETS, [
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-x86_64",
    "windows-x86_64",
  ]);
});

test("builds stable sorted updater JSON for a complete matrix", () => {
  const manifest = aggregateManifest({
    fragments: completeFragments().reverse(),
    tag: "v1.6.2",
    commit: commitSha,
    repository: "hccake/skill-deck",
  });

  assert.equal(manifest.version, "1.6.2");
  assert.deepEqual(Object.keys(manifest.platforms), REQUIRED_TARGETS);
  assert.equal(
    manifest.platforms["windows-x86_64"].url,
    "https://github.com/hccake/skill-deck/releases/download/v1.6.2/skill-deck_1.6.2_windows_x86_64.nsis.zip",
  );
  assert.equal(
    manifest.platforms["linux-x86_64"].signature,
    "signature-linux-x86_64",
  );
});

for (const [name, mutate, expected] of [
  [
    "duplicate platform",
    (items) => items.push({ ...items[0] }),
    /duplicate platform/i,
  ],
  ["missing target", (items) => items.pop(), /missing required platform/i],
  [
    "missing signature",
    (items) => {
      items[0].signature = "";
    },
    /signature/i,
  ],
  [
    "missing architecture",
    (items) => {
      items[0].architecture = "";
    },
    /architecture/i,
  ],
  [
    "mismatched version",
    (items) => {
      items[0].version = "1.6.1";
    },
    /version/i,
  ],
  [
    "mismatched tag",
    (items) => {
      items[0].tag = "v1.6.1";
    },
    /tag/i,
  ],
  [
    "mismatched commit",
    (items) => {
      items[0].commitSha = "f".repeat(40);
    },
    /commit/i,
  ],
  [
    "stale asset filename",
    (items) => {
      items[0].assetName = "skill-deck_1.6.1_darwin_aarch64.tar.gz";
      items[0].signatureName = `${items[0].assetName}.sig`;
    },
    /asset filename/i,
  ],
  [
    "unsafe asset URL input",
    (items) => {
      items[0].url = "https://attacker.invalid/file";
    },
    /url/i,
  ],
]) {
  test(`rejects ${name} before producing a manifest`, () => {
    const fragments = completeFragments();
    mutate(fragments);
    assert.throws(
      () =>
        aggregateManifest({
          fragments,
          tag: "v1.6.2",
          commit: commitSha,
          repository: "hccake/skill-deck",
        }),
      expected,
    );
  });
}

test("does not mutate fragment inputs while validating", () => {
  const fragments = completeFragments();
  const before = structuredClone(fragments);
  aggregateManifest({
    fragments,
    tag: "v1.6.2",
    commit: commitSha,
    repository: "hccake/skill-deck",
  });
  assert.deepEqual(fragments, before);
});

test("binds every metadata fragment to non-empty sibling installer and signature files", async () => {
  const root = await mkdtemp(path.join(process.cwd(), ".tmp-skill-deck-release-"));
  try {
    for (const fragmentValue of completeFragments()) {
      const directory = path.join(root, fragmentValue.platform);
      await mkdir(directory, { recursive: true });
      await writeFile(
        path.join(directory, "metadata.json"),
        JSON.stringify(fragmentValue),
      );
      await writeFile(path.join(directory, fragmentValue.assetName), "installer");
      await writeFile(
        path.join(directory, fragmentValue.signatureName),
        fragmentValue.signature,
      );
    }

    assert.deepEqual(await readArtifactFragments(root), completeFragments());
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("rejects missing, empty, or mismatched sibling release files before aggregation", async () => {
  for (const failure of ["missing", "empty", "mismatched"]) {
    const root = await mkdtemp(path.join(process.cwd(), ".tmp-skill-deck-release-"));
    try {
      const fragmentValue = completeFragments()[0];
      await writeFile(
        path.join(root, "metadata.json"),
        JSON.stringify(fragmentValue),
      );
      if (failure !== "missing") {
        await writeFile(
          path.join(root, fragmentValue.assetName),
          failure === "empty" ? "" : "installer",
        );
      }
      await writeFile(
        path.join(root, fragmentValue.signatureName),
        failure === "mismatched" ? "other-signature" : fragmentValue.signature,
      );

      await assert.rejects(() => readArtifactFragments(root), /asset|signature/i);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
});
