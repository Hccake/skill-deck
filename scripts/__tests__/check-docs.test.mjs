import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { checkDocumentation } from "../check-docs.mjs";

async function withDocs(files, run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-docs-"));
  try {
    for (const [relativePath, content] of Object.entries(files)) {
      const filePath = path.join(root, relativePath);
      await mkdir(path.dirname(filePath), { recursive: true });
      await writeFile(filePath, content);
    }
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("validates local files and GitHub-style mixed-language anchors", async () => {
  await withDocs({
    "README.md": [
      "# Entry",
      "",
      "See [discovery](./docs/guide.md#sourcediscovery-与-well-known).",
      "External [documentation](https://example.com/docs) is outside this check.",
    ].join("\n"),
    "docs/guide.md": "# Guide\n\n## SourceDiscovery 与 Well-known\n",
  }, async (root) => {
    assert.deepEqual(await checkDocumentation(root, ["README.md", "docs/guide.md"]), []);
  });
});

test("reports missing local files and anchors with their source locations", async () => {
  await withDocs({
    "README.md": [
      "# Entry",
      "",
      "[missing file](./docs/missing.md)",
      "[missing anchor](./docs/guide.md#not-present)",
    ].join("\n"),
    "docs/guide.md": "# Guide\n",
  }, async (root) => {
    const problems = await checkDocumentation(root, ["README.md", "docs/guide.md"]);
    assert.equal(problems.length, 2);
    assert.match(problems[0], /README\.md:3: missing file: docs\/missing\.md/);
    assert.match(problems[1], /README\.md:4: missing anchor: docs\/guide\.md#not-present/);
  });
});
