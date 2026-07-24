import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const workflowUrl = new URL(
  "../../.github/workflows/release.yml",
  import.meta.url,
);
const ciWorkflowUrl = new URL("../../.github/workflows/ci.yml", import.meta.url);
const packagerUrl = new URL("../package-updater-artifact.mjs", import.meta.url);
const releaseVerifierUrl = new URL("../verify-release-assets.mjs", import.meta.url);

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
    /build-artifacts:[\s\S]*pnpm bindings:check[\s\S]*pnpm lint[\s\S]*pnpm test[\s\S]*pnpm build[\s\S]*cargo test --locked[\s\S]*tauri-action@[0-9a-f]{40}/,
  );
  assert.match(workflow, /build-artifacts:[\s\S]*package smoke/i);
  assert.doesNotMatch(
    workflow.match(/build-artifacts:[\s\S]*?(?=\n  aggregate:)/)?.[0] ?? "",
    /gh release|releaseDraft|uploadUpdaterJson|tagName:/,
  );
  assert.match(workflow, /aggregate:[\s\S]*aggregate-updater-manifest\.mjs/);
});

test("release workflow builds exactly one Tauri v2 updater bundle per platform", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const build = workflow.match(/\n  build-artifacts:[\s\S]*?(?=\n  aggregate:)/)?.[0] ?? "";

  assert.match(build, /args: --target aarch64-apple-darwin --bundles app/);
  assert.match(build, /args: --target x86_64-apple-darwin --bundles app/);
  assert.match(build, /args: --bundles appimage/);
  assert.match(build, /args: --bundles nsis/);
  assert.doesNotMatch(build, /mapfile\b/);
  assert.match(build, /id: tauri-build/);
  assert.match(build, /TAURI_ARTIFACT_PATHS: \$\{\{ steps\.tauri-build\.outputs\.artifactPaths \}\}/);
  assert.match(build, /ref: \$\{\{ github\.workflow_sha \}\}/);
  assert.match(build, /path: \.release-tooling/);
  assert.match(build, /node \.release-tooling\/scripts\/package-updater-artifact\.mjs/);
});

test("release helpers come from the workflow commit instead of the tagged application commit", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const build = workflow.match(/\n  build-artifacts:[\s\S]*?(?=\n  aggregate:)/)?.[0] ?? "";
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  for (const job of [build, aggregate]) {
    assert.match(job, /name: Checkout release tooling/);
    assert.match(job, /ref: \$\{\{ github\.workflow_sha \}\}/);
    assert.match(job, /path: \.release-tooling/);
    assert.match(job, /sparse-checkout: scripts/);
  }
  assert.match(aggregate, /node \.release-tooling\/scripts\/aggregate-updater-manifest\.mjs/);
});

test("release mutation is fail-closed, idempotent for drafts, and publishes latest.json last", async () => {
  const [workflow, verifier] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(releaseVerifierUrl, "utf8"),
  ]);
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(aggregate, /isDraft/);
  assert.match(verifier, /published Release/i);
  assert.match(aggregate, /gh release create/);
  assert.match(aggregate, /gh release upload[\s\S]*--clobber/);
  assert.match(
    aggregate,
    /Verify remote assets[\s\S]*Upload latest\.json last/,
  );
  assert.doesNotMatch(aggregate, /gh release upload[^\n]*metadata/i);
});

test("release aggregation uses tested scripts instead of inline Node heredocs", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.doesNotMatch(aggregate, /node\s+-\s+<<|<<['\"]?NODE/);
  assert.match(
    aggregate,
    /node \.release-tooling\/scripts\/verify-release-assets\.mjs[\s\S]*--mode draft/,
  );
  assert.match(
    aggregate,
    /node \.release-tooling\/scripts\/verify-release-assets\.mjs[\s\S]*--mode uploaded/,
  );
});

test("release workflow binds tag, package version, and artifacts to one commit SHA", async () => {
  const [workflow, packager] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(packagerUrl, "utf8"),
  ]);

  assert.match(workflow, /git rev-list -n 1/);
  assert.match(workflow, /EXPECTED_TAG="v\$\{VERSION\}"/);
  assert.match(workflow, /commit_sha/);
  assert.match(workflow, /metadata\.json/);
  assert.match(workflow, /package-updater-artifact\.mjs/);
  assert.match(packager, /signatureName/);
  assert.match(packager, /commitSha/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:package\.json"/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:CHANGELOG\.md"/);
});

test("release workflow derives and enforces GitHub prerelease state from the version", async () => {
  const [workflow, verifier] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(releaseVerifierUrl, "utf8"),
  ]);
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(
    workflow,
    /prerelease: \$\{\{ steps\.release\.outputs\.prerelease \}\}/,
  );
  assert.match(workflow, /if \[\[ "\$VERSION" == \*-\* \]\]/);
  assert.match(workflow, /echo "prerelease=\$PRERELEASE"/);
  assert.match(aggregate, /--json isDraft,isPrerelease,assets/);
  assert.match(aggregate, /EXPECTED_PRERELEASE/);
  assert.match(verifier, /Release prerelease state does not match version/);
  assert.match(aggregate, /PRERELEASE_ARGS=\(\)/);
  assert.match(aggregate, /PRERELEASE_ARGS\+=\(--prerelease\)/);
  assert.match(aggregate, /gh release create[\s\S]*"\$\{PRERELEASE_ARGS\[@\]\}"/);
});

test("release verification rejects stale managed assets and verifies the final manifest", async () => {
  const [workflow, verifier] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(releaseVerifierUrl, "utf8"),
  ]);
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(verifier, /unexpected managed updater asset/i);
  assert.match(aggregate, /Verify uploaded latest\.json/);
  assert.ok(
    aggregate.indexOf("--mode draft")
      < aggregate.indexOf('gh release upload "$TAG" "${FILES[@]}" --clobber'),
    "unexpected managed updater assets must be rejected before the first upload",
  );
});
