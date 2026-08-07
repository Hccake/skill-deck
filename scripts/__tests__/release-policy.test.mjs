import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { parse } from "yaml";

const workflowUrl = new URL(
  "../../.github/workflows/release.yml",
  import.meta.url,
);
const ciWorkflowUrl = new URL("../../.github/workflows/ci.yml", import.meta.url);
const qualityWorkflowUrl = new URL(
  "../../.github/workflows/quality.yml",
  import.meta.url,
);
const packagerUrl = new URL("../package-updater-artifact.mjs", import.meta.url);
const releaseVerifierUrl = new URL("../verify-release-assets.mjs", import.meta.url);

const readWorkflow = async (url) => parse(await readFile(url, "utf8"));

test("CI and Release run the same quality workflow for an exact commit", async () => {
  const [quality, ci, release] = await Promise.all([
    readWorkflow(qualityWorkflowUrl),
    readWorkflow(ciWorkflowUrl),
    readWorkflow(workflowUrl),
  ]);

  assert.equal(quality.on.workflow_call.inputs.target_sha.required, true);
  assert.equal(ci.jobs.quality.uses, "./.github/workflows/quality.yml");
  assert.equal(ci.jobs.quality.with.target_sha, "${{ github.sha }}");
  assert.equal(release.jobs.quality.uses, "./.github/workflows/quality.yml");
  assert.equal(
    release.jobs.quality.with.target_sha,
    "${{ needs.validate.outputs.commit_sha }}",
  );
  assert.deepEqual(release.jobs["build-artifacts"].needs, [
    "validate",
    "quality",
  ]);

  for (const [jobName, job] of Object.entries(quality.jobs)) {
    const checkout = job.steps?.find((step) =>
      step.uses?.startsWith("actions/checkout@"),
    );
    if (jobName === "quality-gate") {
      assert.equal(checkout, undefined);
    } else {
      assert.equal(
        checkout?.with?.ref,
        "${{ inputs.target_sha }}",
        `${jobName} must validate the requested commit`,
      );
    }
  }
});

test("quality workflow separates portable formatting, static checks, and tests", async () => {
  const workflow = await readWorkflow(qualityWorkflowUrl);
  const expectedPlatforms = [
    "ubuntu-22.04",
    "windows-latest",
    "macos-latest",
  ];

  assert.equal(workflow.jobs["rust-format"]["runs-on"], "ubuntu-22.04");
  assert.deepEqual(
    workflow.jobs["rust-static"].strategy.matrix.os,
    expectedPlatforms,
  );
  assert.deepEqual(
    workflow.jobs["rust-test"].strategy.matrix.os,
    expectedPlatforms,
  );
  assert.equal(workflow.jobs["rust-static"].strategy["fail-fast"], false);
  assert.equal(workflow.jobs["rust-test"].strategy["fail-fast"], false);

  const staticCommands = workflow.jobs["rust-static"].steps
    .map((step) => step.run ?? "")
    .join("\n");
  const testCommands = workflow.jobs["rust-test"].steps
    .map((step) => step.run ?? "")
    .join("\n");
  assert.match(staticCommands, /cargo check[^\n]*--locked[^\n]*--all-targets/);
  assert.match(
    staticCommands,
    /cargo clippy[^\n]*--locked[^\n]*--all-targets[^\n]*-- -D warnings/,
  );
  assert.doesNotMatch(staticCommands, /cargo test/);
  assert.match(testCommands, /cargo test[^\n]*--locked/);
  assert.doesNotMatch(testCommands, /cargo (?:check|clippy)/);

  const clippyStep = workflow.jobs["rust-static"].steps.find((step) =>
    (step.run ?? "").includes("cargo clippy"),
  );
  assert.equal(clippyStep.if, "${{ always() }}");
  for (const jobName of ["rust-format", "rust-static", "rust-test"]) {
    const upload = workflow.jobs[jobName].steps.find(
      (step) => step.uses === "actions/upload-artifact@v4",
    );
    assert.equal(upload.if, "${{ always() }}");
  }

  assert.deepEqual(workflow.jobs["quality-gate"].needs, [
    "msrv",
    "frontend",
    "workflow-lint",
    "shellcheck",
    "rust-format",
    "rust-static",
    "rust-test",
  ]);
  assert.equal(workflow.jobs["quality-gate"].if, "${{ always() }}");
});

test("quality workflow validates GitHub Actions syntax", async () => {
  const workflow = await readWorkflow(qualityWorkflowUrl);
  const lintJob = workflow.jobs["workflow-lint"];

  assert.equal(lintJob["runs-on"], "ubuntu-22.04");
  assert.ok(
    lintJob.steps.some(
      (step) => step.uses === "docker://rhysd/actionlint:1.7.12",
    ),
  );
});

test("release workflow keeps platform jobs artifact-only and uses one aggregator", async () => {
  const workflow = await readWorkflow(workflowUrl);
  const artifactJob = workflow.jobs["build-artifacts"];
  const artifactCommands = artifactJob.steps
    .map((step) => step.run ?? "")
    .join("\n");

  assert.deepEqual(artifactJob.needs, ["validate", "quality"]);
  assert.doesNotMatch(
    artifactCommands,
    /pnpm (?:bindings:check|lint|test|build)|cargo (?:check|clippy|test)/,
  );
  assert.ok(
    artifactJob.steps.some(
      (step) =>
        typeof step.uses === "string" &&
        step.uses.startsWith("tauri-apps/tauri-action@"),
    ),
  );
  assert.ok(
    artifactJob.steps.some((step) =>
      (step.run ?? "").includes("package-updater-artifact.mjs"),
    ),
  );
  assert.ok(workflow.jobs.aggregate);
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
  assert.match(aggregate, /gh release edit/);
  assert.match(aggregate, /gh release upload[\s\S]*--clobber/);
  assert.match(
    aggregate,
    /Verify remote assets[\s\S]*Upload latest\.json last/,
  );
  assert.doesNotMatch(aggregate, /gh release upload[^\n]*metadata/i);
});

test("release body is synchronized from the tagged changelog section", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const aggregate = workflow.match(/\n  aggregate:[\s\S]*/)?.[0] ?? "";

  assert.match(
    aggregate,
    /node \.release-tooling\/scripts\/extract-release-notes\.mjs[\s\S]*--changelog CHANGELOG\.md[\s\S]*--version "\$\{\{ needs\.validate\.outputs\.version \}\}"[\s\S]*--output release-notes\.md/,
  );
  assert.doesNotMatch(aggregate, /--generate-notes/);
  assert.match(
    aggregate,
    /gh release create[\s\S]*--notes-file release-notes\.md/,
  );
  assert.match(
    aggregate,
    /gh release edit[\s\S]*--notes-file release-notes\.md/,
  );
  assert.match(
    aggregate,
    /Verify Release notes[\s\S]*gh release view[\s\S]*--json body[\s\S]*cmp --silent release-notes\.md/,
  );
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
