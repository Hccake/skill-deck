import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildWorkerManifest,
  parsePrepareArgs,
  verifyWorkerArtifact,
  workerBuildInvocation,
} from "../prepare-wsl-worker.mjs";

test("Linux builds the fixed musl worker target directly", () => {
  const args = parsePrepareArgs([], "linux");

  assert.deepEqual(args, { mode: "build", distro: undefined });
  assert.deepEqual(
    workerBuildInvocation({ platform: "linux", distro: args.distro }),
    {
      command: "cargo",
      args: [
        "build",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "-p",
        "wsl-environment-worker",
        "--target",
        "x86_64-unknown-linux-musl",
        "--release",
      ],
    },
  );
});

test("Windows builds through the selected WSL distribution", () => {
  const args = parsePrepareArgs(
    ["--", "--distro", "Ubuntu-24.04"],
    "win32",
  );
  assert.deepEqual(args, { mode: "build", distro: "Ubuntu-24.04" });

  assert.deepEqual(
    workerBuildInvocation({
      platform: "win32",
      distro: args.distro,
      linuxRepositoryRoot: "/mnt/c/code/skill-deck",
    }),
    {
      command: "wsl.exe",
      args: [
        "--distribution",
        "Ubuntu-24.04",
        "--cd",
        "/mnt/c/code/skill-deck",
        "--exec",
        "/bin/sh",
        "-lc",
        'exec "$@"',
        "--",
        "cargo",
        "build",
        "--locked",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "-p",
        "wsl-environment-worker",
        "--target",
        "x86_64-unknown-linux-musl",
        "--release",
      ],
    },
  );
  assert.throws(
    () => parsePrepareArgs([], "win32"),
    /--distro is required on Windows/,
  );
});

test("worker manifest uses one SHA-256 value as its build identity", () => {
  assert.deepEqual(
    buildWorkerManifest("ab".repeat(32)),
    {
      buildId: `sha256:${"ab".repeat(32)}`,
      sha256: `sha256:${"ab".repeat(32)}`,
      target: "x86_64-unknown-linux-musl",
    },
  );
});

test("worker artifact verification rejects every mismatched contract field", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "skill-deck-worker-"));
  const directory = path.join(root, "current");
  await mkdir(directory);
  const digest = "87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e";
  const manifest = buildWorkerManifest(digest);
  try {
    await writeFile(path.join(directory, "worker"), "worker");
    await writeFile(
      path.join(directory, "manifest.json"),
      `${JSON.stringify(manifest)}\n`,
    );
    assert.deepEqual(await verifyWorkerArtifact(directory), manifest);

    await writeFile(
      path.join(directory, "manifest.json"),
      `${JSON.stringify({ ...manifest, target: "x86_64-unknown-linux-gnu" })}\n`,
    );
    await assert.rejects(() => verifyWorkerArtifact(directory), /target/);

    await writeFile(path.join(directory, "manifest.json"), "not-json");
    await assert.rejects(() => verifyWorkerArtifact(directory), /JSON/);

    await writeFile(
      path.join(directory, "manifest.json"),
      `${JSON.stringify({ ...manifest, buildId: `sha256:${"a".repeat(64)}` })}\n`,
    );
    await assert.rejects(() => verifyWorkerArtifact(directory), /buildId/);

    await writeFile(
      path.join(directory, "manifest.json"),
      `${JSON.stringify(manifest)}\n`,
    );
    await writeFile(path.join(directory, "worker"), "damaged");
    await assert.rejects(() => verifyWorkerArtifact(directory), /bytes/);
    await rm(path.join(directory, "worker"));
    await assert.rejects(() => verifyWorkerArtifact(directory));
    await writeFile(path.join(directory, "worker"), "worker");
    await rm(path.join(directory, "manifest.json"));
    await assert.rejects(() => verifyWorkerArtifact(directory));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("verify mode is independent from the Windows build distro", () => {
  assert.deepEqual(
    parsePrepareArgs(["--verify", "C:\\artifact"], "win32"),
    { mode: "verify", directory: "C:\\artifact" },
  );
});
