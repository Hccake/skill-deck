import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { parse } from "yaml";

import {
  documentedInstallerNames,
  expectedReleaseAssetNames,
} from "../verify-release-assets.mjs";

const workflowUrl = new URL(
  "../../.github/workflows/release.yml",
  import.meta.url,
);
const ciWorkflowUrl = new URL("../../.github/workflows/ci.yml", import.meta.url);
const qualityWorkflowUrl = new URL(
  "../../.github/workflows/quality.yml",
  import.meta.url,
);
const releaseVerifierUrl = new URL("../verify-release-assets.mjs", import.meta.url);
const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));

const readWorkflow = async (url) => parse(await readFile(url, "utf8"));

test("Tauri package exposes only the desktop application binary", () => {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
        "--manifest-path",
        "src-tauri/Cargo.toml",
      ],
      { cwd: repositoryRoot, encoding: "utf8" },
    ),
  );
  const app = metadata.packages.find(
    (entry) => entry.manifest_path
      .replaceAll("\\", "/")
      .endsWith("/src-tauri/Cargo.toml"),
  );
  const binaries = app.targets
    .filter((target) => target.kind.includes("bin"))
    .map((target) => target.name)
    .sort();

  assert.deepEqual(binaries, ["app"]);
});

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
  assert.deepEqual(release.jobs["build-release"].needs, [
    "validate",
    "quality",
    "prepare-release",
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

test("quality workflow validates GitHub Actions syntax and documentation", async () => {
  const workflow = await readWorkflow(qualityWorkflowUrl);
  const lintJob = workflow.jobs["workflow-lint"];
  assert.equal(lintJob["runs-on"], "ubuntu-22.04");
  assert.ok(
    lintJob.steps.some(
      (step) => step.uses === "docker://rhysd/actionlint:1.7.12",
    ),
  );

  const frontendCommands = workflow.jobs.frontend.steps
    .map((step) => step.run ?? "")
    .filter(Boolean);
  assert.ok(frontendCommands.includes("pnpm docs:check"));
});

test("README installation examples match the release artifact contract", async () => {
  const [english, chinese] = await Promise.all([
    readFile(new URL("../../README.md", import.meta.url), "utf8"),
    readFile(new URL("../../README.zh-CN.md", import.meta.url), "utf8"),
  ]);

  for (const installer of documentedInstallerNames("x.x.x")) {
    assert.match(english, new RegExp(installer.replaceAll(".", "\\.")));
    assert.match(chinese, new RegExp(installer.replaceAll(".", "\\.")));
  }
});

test("release workflow prepares one draft and lets tauri-action upload each platform", async () => {
  const workflow = await readWorkflow(workflowUrl);
  const build = workflow.jobs["build-release"];
  const commands = build.steps.map((step) => step.run ?? "").join("\n");

  assert.ok(workflow.jobs["prepare-release"]);
  assert.ok(workflow.jobs["verify-release"]);
  assert.equal(workflow.jobs["verify-release"].permissions.contents, "write");
  assert.deepEqual(build.needs, ["validate", "quality", "prepare-release"]);
  assert.equal(build.permissions.contents, "write");
  assert.doesNotMatch(
    commands,
    /pnpm (?:bindings:check|lint|test|build)|cargo (?:check|clippy|test)/,
  );

  const action = build.steps.find((step) =>
    step.uses?.startsWith("tauri-apps/tauri-action@"),
  );
  assert.ok(action);
  assert.equal(
    action.with.releaseId,
    "${{ needs.prepare-release.outputs.release_id }}",
  );
  assert.equal(
    action.with.releaseBody,
    "${{ needs.prepare-release.outputs.release_body }}",
  );
  assert.equal(action.with.uploadUpdaterJson, true);
  assert.equal(action.with.uploadUpdaterSignatures, true);
  assert.equal(action.with.updaterJsonPreferNsis, false);
  assert.equal(action.with.retryAttempts, 3);
  assert.equal(
    action.with.releaseAssetNamePattern,
    "skill-deck_[version]_${{ matrix.asset_platform }}_[arch][setup][ext]",
  );
  assert.doesNotMatch(commands, /package-updater-artifact\.mjs/);
  assert.equal(workflow.jobs.aggregate, undefined);
});

test("release workflow omits MSI for prereleases and retains it for stable releases", async () => {
  const workflow = await readWorkflow(workflowUrl);
  const build = workflow.jobs["build-release"];
  const windows = build.strategy.matrix.include.find(
    (entry) => entry.runner === "windows-latest",
  );
  const action = build.steps.find((step) =>
    step.uses?.startsWith("tauri-apps/tauri-action@"),
  );

  assert.equal(windows.args, "--bundles nsis,msi");
  assert.equal(windows.prerelease_args, "--bundles nsis");
  assert.equal(
    action.with.args,
    "${{ needs.validate.outputs.prerelease == 'true' && matrix.prerelease_args || matrix.args }}",
  );
});

test("release helpers come from the workflow commit instead of the tagged application commit", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const prepare =
    workflow.match(/\n  prepare-release:[\s\S]*?(?=\n  build-release:)/)?.[0] ??
    "";
  const verify = workflow.match(/\n  verify-release:[\s\S]*/)?.[0] ?? "";

  for (const job of [prepare, verify]) {
    assert.match(job, /name: Checkout release tooling/);
    assert.match(job, /ref: \$\{\{ github\.workflow_sha \}\}/);
    assert.match(job, /path: \.release-tooling/);
    assert.match(job, /sparse-checkout: scripts/);
  }
  assert.doesNotMatch(workflow, /aggregate-updater-manifest\.mjs/);
  assert.doesNotMatch(workflow, /package-updater-artifact\.mjs/);
});

test("release mutation is fail-closed and idempotent for drafts", async () => {
  const [workflow, verifier] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(releaseVerifierUrl, "utf8"),
  ]);
  const prepare =
    workflow.match(/\n  prepare-release:[\s\S]*?(?=\n  build-release:)/)?.[0] ??
    "";

  assert.match(prepare, /isDraft/);
  assert.match(verifier, /published Release/i);
  assert.match(prepare, /gh release create/);
  assert.match(prepare, /gh release edit/);
  assert.doesNotMatch(prepare, /gh release upload/);
});

test("release body is synchronized from the tagged changelog section", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const prepare =
    workflow.match(/\n  prepare-release:[\s\S]*?(?=\n  build-release:)/)?.[0] ??
    "";

  assert.match(
    prepare,
    /node \.release-tooling\/scripts\/extract-release-notes\.mjs[\s\S]*--changelog CHANGELOG\.md[\s\S]*--version "\$\{\{ needs\.validate\.outputs\.version \}\}"[\s\S]*--output release-notes\.md/,
  );
  assert.doesNotMatch(prepare, /--generate-notes/);
  assert.match(prepare, /gh release create[\s\S]*--notes-file release-notes\.md/);
  assert.match(prepare, /gh release edit[\s\S]*--notes-file release-notes\.md/);
  assert.match(
    prepare,
    /Verify Release notes[\s\S]*gh release view[\s\S]*--json body[\s\S]*cmp --silent release-notes\.md/,
  );
});

test("final release verification uses tested scripts instead of inline Node heredocs", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const prepare =
    workflow.match(/\n  prepare-release:[\s\S]*?(?=\n  build-release:)/)?.[0] ??
    "";
  const verify = workflow.match(/\n  verify-release:[\s\S]*/)?.[0] ?? "";

  assert.doesNotMatch(verify, /node\s+-\s+<<|<<['"]?NODE/);
  assert.match(
    prepare,
    /node \.release-tooling\/scripts\/verify-release-assets\.mjs[\s\S]*--mode draft/,
  );
  assert.match(
    verify,
    /node \.release-tooling\/scripts\/verify-release-assets\.mjs[\s\S]*--mode complete/,
  );
});

test("release workflow binds tag, package version, and artifacts to one commit SHA", async () => {
  const workflow = await readFile(workflowUrl, "utf8");

  assert.match(workflow, /git rev-list -n 1/);
  assert.match(workflow, /EXPECTED_TAG="v\$\{VERSION\}"/);
  assert.match(workflow, /commit_sha/);
  assert.match(workflow, /ref: \$\{\{ needs\.validate\.outputs\.commit_sha \}\}/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:package\.json"/);
  assert.match(workflow, /git show "\$\{COMMIT_SHA\}:CHANGELOG\.md"/);
});

test("release workflow derives and enforces GitHub prerelease state from the version", async () => {
  const [workflow, verifier] = await Promise.all([
    readFile(workflowUrl, "utf8"),
    readFile(releaseVerifierUrl, "utf8"),
  ]);
  const prepare =
    workflow.match(/\n  prepare-release:[\s\S]*?(?=\n  build-release:)/)?.[0] ??
    "";

  assert.match(
    workflow,
    /prerelease: \$\{\{ steps\.release\.outputs\.prerelease \}\}/,
  );
  assert.match(workflow, /if \[\[ "\$VERSION" == \*-\* \]\]/);
  assert.match(workflow, /echo "prerelease=\$PRERELEASE"/);
  assert.match(prepare, /--json [^\n]*isDraft[^\n]*isPrerelease/);
  assert.match(prepare, /EXPECTED_PRERELEASE/);
  assert.match(verifier, /Release prerelease state does not match version/);
  assert.match(prepare, /PRERELEASE_ARGS=\(\)/);
  assert.match(prepare, /PRERELEASE_ARGS\+=\(--prerelease\)/);
  assert.match(
    prepare,
    /gh release create[\s\S]*"\$\{PRERELEASE_ARGS\[@\]\}"/,
  );
});

test("release verification checks the complete remote asset and updater manifest contract", async () => {
  const workflow = await readFile(workflowUrl, "utf8");
  const verify = workflow.match(/\n  verify-release:[\s\S]*/)?.[0] ?? "";

  assert.equal(expectedReleaseAssetNames("1.7.0-beta.4").length, 15);
  assert.equal(expectedReleaseAssetNames("1.7.0").length, 17);
  assert.match(verify, /gh release download[\s\S]*latest\.json/);
  assert.match(verify, /gh release download[\s\S]*\*\.sig/);
  assert.match(verify, /--mode complete/);
});
