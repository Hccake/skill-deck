import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { extractReleaseNotes } from "../extract-release-notes.mjs";

const scriptPath = fileURLToPath(
  new URL("../extract-release-notes.mjs", import.meta.url),
);

const changelog = `# Changelog\r
\r
## [Unreleased]\r
\r
## [1.7.0] - 2026-08-20\r
\r
### Changed\r
\r
- Stable change.\r
\r
## [1.7.0-beta.3] - 2026-08-07\r
\r
### Changed\r
\r
- Beta change.\r
\r
### Fixed\r
\r
- Beta fix.\r
\r
## [1.7.0-beta.2] - 2026-07-27\r
\r
- Previous beta.\r
`;

test("extracts only the exact version body with normalized line endings", () => {
  assert.equal(
    extractReleaseNotes(changelog, "1.7.0-beta.3"),
    "### Changed\n\n- Beta change.\n\n### Fixed\n\n- Beta fix.\n",
  );
  assert.equal(
    extractReleaseNotes(changelog, "1.7.0"),
    "### Changed\n\n- Stable change.\n",
  );
});

test("rejects missing, duplicate, and empty release sections", () => {
  assert.throws(
    () => extractReleaseNotes(changelog, "1.8.0"),
    /missing changelog section for 1\.8\.0/i,
  );
  assert.throws(
    () => extractReleaseNotes(`${changelog}\n## [1.7.0-beta.3]\n\n- Duplicate.\n`, "1.7.0-beta.3"),
    /duplicate changelog sections for 1\.7\.0-beta\.3/i,
  );
  assert.throws(
    () => extractReleaseNotes("## [1.7.0-beta.3]\n\n## [1.7.0-beta.2]\n", "1.7.0-beta.3"),
    /empty changelog section for 1\.7\.0-beta\.3/i,
  );
});

test("writes the extracted notes through the CLI contract used by Release", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-release-notes-"));
  const changelogPath = path.join(root, "CHANGELOG.md");
  const outputPath = path.join(root, "release-notes.md");
  try {
    await writeFile(changelogPath, changelog);
    execFileSync(process.execPath, [
      scriptPath,
      "--changelog",
      changelogPath,
      "--version",
      "1.7.0-beta.3",
      "--output",
      outputPath,
    ]);

    assert.equal(
      await readFile(outputPath, "utf8"),
      "### Changed\n\n- Beta change.\n\n### Fixed\n\n- Beta fix.\n",
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
