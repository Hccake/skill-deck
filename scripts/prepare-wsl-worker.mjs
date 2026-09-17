import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const WORKER_TARGET = "x86_64-unknown-linux-musl";
const WORKER_PACKAGE = "wsl-environment-worker";
const BUILD_ARGS = [
  "build",
  "--locked",
  "--manifest-path",
  "src-tauri/Cargo.toml",
  "-p",
  WORKER_PACKAGE,
  "--target",
  WORKER_TARGET,
  "--release",
];

export function parsePrepareArgs(argv, platform = process.platform) {
  if (argv[0] === "--") argv = argv.slice(1);
  if (argv[0] === "--verify" && argv.length === 2) {
    return { mode: "verify", directory: argv[1] };
  }
  let distro;
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] !== "--distro" || !argv[index + 1]) {
      throw new Error(`unknown or incomplete argument: ${argv[index]}`);
    }
    distro = argv[index + 1];
    index += 1;
  }
  if (platform === "win32" && !distro) {
    throw new Error("--distro is required on Windows");
  }
  return { mode: "build", distro };
}

export function workerBuildInvocation({
  platform,
  distro,
  linuxRepositoryRoot,
}) {
  if (platform === "linux") {
    return { command: "cargo", args: BUILD_ARGS };
  }
  if (platform === "win32" && distro && linuxRepositoryRoot) {
    return {
      command: "wsl.exe",
      args: [
        "--distribution",
        distro,
        "--cd",
        linuxRepositoryRoot,
        "--exec",
        "/bin/sh",
        "-lc",
        'exec "$@"',
        "--",
        "cargo",
        ...BUILD_ARGS,
      ],
    };
  }
  throw new Error(`unsupported worker build platform: ${platform}`);
}

export function buildWorkerManifest(hexDigest) {
  const sha256 = `sha256:${hexDigest}`;
  return { buildId: sha256, sha256, target: WORKER_TARGET };
}

export async function verifyWorkerArtifact(directory) {
  const worker = await readFile(path.join(directory, "worker"));
  const manifest = JSON.parse(
    await readFile(path.join(directory, "manifest.json"), "utf8"),
  );
  if (manifest.target !== WORKER_TARGET) {
    throw new Error(`unsupported WSL worker target: ${manifest.target}`);
  }
  if (manifest.buildId !== manifest.sha256) {
    throw new Error("WSL worker buildId does not match sha256");
  }
  const actual = `sha256:${createHash("sha256").update(worker).digest("hex")}`;
  if (manifest.sha256 !== actual) {
    throw new Error("WSL worker bytes do not match manifest sha256");
  }
  return manifest;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: options.encoding,
    stdio: options.encoding ? ["ignore", "pipe", "inherit"] : "inherit",
    cwd: options.cwd,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
  return result.stdout?.trim();
}

async function prepare() {
  const repositoryRoot = fileURLToPath(new URL("..", import.meta.url));
  const args = parsePrepareArgs(process.argv.slice(2));
  if (args.mode === "verify") {
    await verifyWorkerArtifact(path.resolve(repositoryRoot, args.directory));
    return;
  }
  const { distro } = args;
  const linuxRepositoryRoot =
    process.platform === "win32"
      ? run(
          "wsl.exe",
          [
            "--distribution",
            distro,
            "--exec",
            "wslpath",
            "-a",
            "-u",
            repositoryRoot,
          ],
          { encoding: "utf8" },
        )
      : undefined;
  const invocation = workerBuildInvocation({
    platform: process.platform,
    distro,
    linuxRepositoryRoot,
  });
  run(invocation.command, invocation.args, { cwd: repositoryRoot });

  const builtWorker = path.join(
    repositoryRoot,
    "src-tauri",
    "target",
    WORKER_TARGET,
    "release",
    WORKER_PACKAGE,
  );
  const outputDirectory = path.join(
    repositoryRoot,
    "src-tauri",
    "target",
    "wsl-worker",
    "current",
  );
  const outputWorker = path.join(outputDirectory, "worker");
  await mkdir(outputDirectory, { recursive: true });
  await copyFile(builtWorker, outputWorker);
  await chmod(outputWorker, 0o755);
  const digest = createHash("sha256")
    .update(await readFile(outputWorker))
    .digest("hex");
  await writeFile(
    path.join(outputDirectory, "manifest.json"),
    `${JSON.stringify(buildWorkerManifest(digest), null, 2)}\n`,
  );
  await verifyWorkerArtifact(outputDirectory);
}

if (
  process.argv[1] &&
  fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
) {
  await prepare();
}
