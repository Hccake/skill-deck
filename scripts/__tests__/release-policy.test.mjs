import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const workflowUrl = new URL(
  "../../.github/workflows/release.yml",
  import.meta.url,
);
const ciWorkflowUrl = new URL("../../.github/workflows/ci.yml", import.meta.url);

test("PR CI keeps stable required jobs and enforces the exact Rust gate", async () => {
  const workflow = await readFile(ciWorkflowUrl, "utf8");

  assert.match(workflow, /\n  frontend:/);
  assert.match(workflow, /\n  msrv:/);
  assert.match(workflow, /name: rust \(\$\{\{ matrix\.os \}\}\)/);
  assert.match(workflow, /cargo check[^\n]*--locked[^\n]*--all-targets/);
  assert.match(
    workflow,
    /cargo clippy[^\n]*--locked[^\n]*--all-targets[^\n]*-- -D warnings/,
  );
  assert.match(workflow, /cargo test[^\n]*--locked/);
});

test("release workflow keeps platform jobs artifact-only and uses one aggregator", async () => {
  const workflow = await readFile(workflowUrl, "utf8");

  assert.match(workflow, /jobs:\s*[\s\S]*validate:/);
  assert.match(workflow, /build-artifacts:[\s\S]*strategy:[\s\S]*matrix:/);
  assert.match(
    workflow,
    /build-artifacts:[\s\S]*pnpm bindings:check[\s\S]*pnpm lint[\s\S]*pnpm test[\s\S]*pnpm build[\s\S]*cargo test --locked[\s\S]*tauri-action@v1/,
  );
  assert.match(workflow, /build-artifacts:[\s\S]*package smoke/i);
  assert.doesNotMatch(
    workflow.match(/build-artifacts:[\s\S]*?(?=\n  aggregate:)/)?.[0] ?? "",
    /gh release|releaseDraft|uploadUpdaterJson|tagName:/,
  );
  assert.match(workflow, /aggregate:[\s\S]*aggregate-updater-manifest\.mjs/);
});

test("release mutation is fail-closed, idempotent for drafts, and publishes latest.json last", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(aggregate, /isDraft/);
  assert.match(aggregate, /published Release/i);
  assert.match(aggregate, /gh release create/);
  assert.match(aggregate, /gh release upload[\s\S]*--clobber/);
  assert.match(
    aggregate,
    /Verify remote assets[\s\S]*Upload latest\.json last/,
  );
  assert.doesNotMatch(aggregate, /gh release upload[^\n]*metadata/i);
});

test("release workflow binds tag, package version, and artifacts to one commit SHA", async () => {
  const workflow = await readFile(workflowUrl, "utf8");

  assert.match(workflow, /git rev-list -n 1/);
  assert.match(workflow, /EXPECTED_TAG="v\$\{VERSION\}"/);
  assert.match(workflow, /commit_sha/);
  assert.match(workflow, /metadata\.json/);
  assert.match(workflow, /signatureName/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:package\.json"/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:CHANGELOG\.md"/);
});

test("release verification rejects stale managed assets and verifies the final manifest", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(aggregate, /unexpected managed updater asset/i);
  assert.match(aggregate, /Verify uploaded latest\.json/);
  assert.ok(
    aggregate.indexOf("Unexpected managed updater asset")
      < aggregate.indexOf('gh release upload "$TAG" "${FILES[@]}" --clobber'),
    "unexpected managed updater assets must be rejected before the first upload",
  );
});
