import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const skillsBinary = join(
  repoRoot,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "skills.cmd" : "skills",
);
const skillName = "interop-skill";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    env: options.env ?? process.env,
    shell: process.platform === "win32" && command.endsWith(".cmd"),
  });

  assert.equal(
    result.status,
    0,
    [
      `${command} ${args.join(" ")} failed with status ${result.status}`,
      result.stdout,
      result.stderr,
    ].filter(Boolean).join("\n"),
  );

  return result;
}

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

async function writeJson(path, value) {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

async function writeSkill(sourceRoot, body) {
  const skillRoot = join(sourceRoot, skillName);
  await mkdir(skillRoot, { recursive: true });
  await writeFile(
    join(skillRoot, "SKILL.md"),
    `---\nname: ${skillName}\ndescription: CLI interoperability fixture\n---\n\n${body}\n`,
    "utf8",
  );
  await writeFile(join(skillRoot, "payload.txt"), `${body}\n`, "utf8");
}

async function createFixture(t) {
  const root = await mkdtemp(join(tmpdir(), "skill-deck-skills-cli-"));
  const home = join(root, "home");
  const project = join(root, "project");
  const source = join(root, "source");

  t.after(async () => {
    await rm(root, { recursive: true, force: true });
  });

  await mkdir(join(project, "agent"), { recursive: true });
  await mkdir(home, { recursive: true });
  await writeJson(join(project, "package.json"), {
    private: true,
    dependencies: { eve: "0.0.0" },
  });
  await writeSkill(source, "version one");

  return { root, home, project, source };
}

function cliEnvironment(home) {
  return {
    ...process.env,
    HOME: home,
    USERPROFILE: home,
    XDG_STATE_HOME: join(home, ".local", "state"),
    DISABLE_TELEMETRY: "1",
    DO_NOT_TRACK: "1",
    FORCE_COLOR: "0",
    NO_COLOR: "1",
  };
}

function runSkills(fixture, args) {
  return run(skillsBinary, args, {
    cwd: fixture.project,
    env: cliEnvironment(fixture.home),
  });
}

function installFrom(fixture, source, { agents = ["eve"], subagents = [] } = {}) {
  const args = [
    "add",
    source,
    "--skill",
    skillName,
    "--agent",
    ...agents,
  ];
  if (subagents.length > 0) {
    args.push("--subagent", ...subagents);
  }
  args.push("--copy", "--yes");
  runSkills(fixture, args);
}

async function readProjectLock(project) {
  return JSON.parse(await readFile(join(project, "skills-lock.json"), "utf8"));
}

function eveSkillPath(project, subagent) {
  return subagent
    ? join(project, "agent", "subagents", subagent, "skills", skillName)
    : join(project, "agent", "skills", skillName);
}

test("uses the pinned Vercel Skills CLI", () => {
  const result = run(skillsBinary, ["--version"], {
    cwd: repoRoot,
    env: cliEnvironment(join(repoRoot, ".tmp-skills-cli-home")),
  });

  assert.equal(result.stdout.trim(), "1.5.23");
});

test("installs Eve root placement without writing redundant metadata", async (t) => {
  const fixture = await createFixture(t);

  installFrom(fixture, fixture.source, { subagents: ["root"] });

  const lock = await readProjectLock(fixture.project);
  assert.equal(await pathExists(join(eveSkillPath(fixture.project), "SKILL.md")), true);
  assert.equal("subagents" in lock.skills[skillName], false);
});

test("records and installs a named Eve subagent", async (t) => {
  const fixture = await createFixture(t);

  installFrom(fixture, fixture.source, { subagents: ["builder"] });

  const lock = await readProjectLock(fixture.project);
  assert.deepEqual(lock.skills[skillName].subagents, ["builder"]);
  assert.equal(
    await pathExists(join(eveSkillPath(fixture.project, "builder"), "SKILL.md")),
    true,
  );
  assert.equal(await pathExists(eveSkillPath(fixture.project)), false);
});

test("preserves multiple Eve targets in CLI order", async (t) => {
  const fixture = await createFixture(t);

  installFrom(fixture, fixture.source, {
    subagents: ["root", "builder", "reviewer"],
  });

  const lock = await readProjectLock(fixture.project);
  assert.deepEqual(lock.skills[skillName].subagents, ["", "builder", "reviewer"]);
  for (const target of [undefined, "builder", "reviewer"]) {
    assert.equal(
      await pathExists(join(eveSkillPath(fixture.project, target), "SKILL.md")),
      true,
    );
  }
});

test("does not create Eve placement when Eve is not targeted", async (t) => {
  const fixture = await createFixture(t);

  installFrom(fixture, fixture.source, { agents: ["claude-code"] });

  const lock = await readProjectLock(fixture.project);
  assert.equal("subagents" in lock.skills[skillName], false);
  assert.equal(
    await pathExists(join(fixture.project, ".claude", "skills", skillName, "SKILL.md")),
    true,
  );
  assert.equal(await pathExists(eveSkillPath(fixture.project)), false);
});

test("replays Eve placement when updating from an offline Git source", async (t) => {
  const fixture = await createFixture(t);
  const gitSource = join(fixture.root, "source-repository.git");
  await writeSkill(gitSource, "version one");
  run("git", ["init"], { cwd: gitSource });
  run("git", ["config", "user.email", "skill-deck@example.invalid"], { cwd: gitSource });
  run("git", ["config", "user.name", "Skill Deck Test"], { cwd: gitSource });
  run("git", ["add", "."], { cwd: gitSource });
  run("git", ["commit", "-m", "fixture: version one"], { cwd: gitSource });

  installFrom(fixture, pathToFileURL(gitSource).href, {
    subagents: ["root", "builder"],
  });

  await writeSkill(gitSource, "version two");
  run("git", ["add", "."], { cwd: gitSource });
  run("git", ["commit", "-m", "fixture: version two"], { cwd: gitSource });
  runSkills(fixture, ["update", "--project", "--yes"]);

  const lock = await readProjectLock(fixture.project);
  assert.deepEqual(lock.skills[skillName].subagents, ["", "builder"]);
  for (const target of [undefined, "builder"]) {
    assert.equal(
      await readFile(join(eveSkillPath(fixture.project, target), "payload.txt"), "utf8"),
      "version two\n",
    );
  }
});
