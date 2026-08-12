import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const rustSourceRoot = fileURLToPath(new URL("../../src-tauri/src/", import.meta.url));
const applicationEntryUrl = new URL("../../src-tauri/src/lib.rs", import.meta.url);

async function rustSourceFiles(root) {
  const entries = await readdir(root, { withFileTypes: true });
  const files = await Promise.all(entries.map(async (entry) => {
    const entryPath = path.join(root, entry.name);
    if (entry.isDirectory()) return rustSourceFiles(entryPath);
    return entry.isFile() && entry.name.endsWith(".rs") ? [entryPath] : [];
  }));
  return files.flat();
}

test("Tauri contexts distinguish test fixtures from the application runtime", async () => {
  const sourceFiles = await rustSourceFiles(rustSourceRoot);
  const ordinaryContexts = [];
  let contextCalls = 0;
  let testContexts = 0;
  for (const sourceFile of sourceFiles) {
    const source = await readFile(sourceFile, "utf8");
    contextCalls += source.match(/tauri::generate_context!\(/g)?.length ?? 0;
    if (source.includes("tauri::generate_context!()")) {
      ordinaryContexts.push(path.relative(rustSourceRoot, sourceFile));
    }
    testContexts += source.match(/tauri::generate_context!\(test\s*=\s*true\)/g)?.length ?? 0;
  }

  assert.deepEqual(ordinaryContexts, ["lib.rs"]);
  assert.ok(testContexts > 0);
  assert.equal(contextCalls, ordinaryContexts.length + testContexts);
  const applicationEntry = await readFile(applicationEntryUrl, "utf8");
  assert.match(applicationEntry, /\.run\(tauri::generate_context!\(\)\)/);
});
