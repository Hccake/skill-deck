import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

export function extractReleaseNotes(changelog, version) {
  invariant(typeof changelog === "string", "Changelog must be text");
  invariant(typeof version === "string" && version.trim(), "Version is required");

  const normalized = changelog.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n");
  const versionHeadings = [];

  for (let index = 0; index < lines.length; index += 1) {
    const match = /^## \[([^\]]+)\](?: - .+)?$/.exec(lines[index]);
    if (match?.[1] === version) versionHeadings.push(index);
  }

  invariant(
    versionHeadings.length > 0,
    `Missing changelog section for ${version}`,
  );
  invariant(
    versionHeadings.length === 1,
    `Duplicate changelog sections for ${version}`,
  );

  const start = versionHeadings[0] + 1;
  let end = lines.length;
  for (let index = start; index < lines.length; index += 1) {
    if (/^## \[[^\]]+\](?: - .+)?$/.test(lines[index])) {
      end = index;
      break;
    }
  }

  const body = lines.slice(start, end);
  while (body[0]?.trim() === "") body.shift();
  while (body.at(-1)?.trim() === "") body.pop();
  invariant(body.some((line) => line.trim()), `Empty changelog section for ${version}`);
  return `${body.join("\n")}\n`;
}

function parseArgs(argv) {
  if (argv.includes("--help")) return { help: true };
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    invariant(
      key?.startsWith("--") && value,
      `Invalid CLI argument near ${key ?? "<end>"}`,
    );
    args[key.slice(2)] = value;
  }
  for (const key of ["changelog", "version", "output"]) {
    invariant(args[key], `Missing --${key}`);
  }
  return args;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(
      "Usage: node scripts/extract-release-notes.mjs --changelog FILE --version VERSION --output FILE",
    );
    return;
  }
  const changelog = await fs.readFile(args.changelog, "utf8");
  await fs.writeFile(args.output, extractReleaseNotes(changelog, args.version));
}

if (
  process.argv[1] &&
  fileURLToPath(import.meta.url) === path.resolve(process.argv[1])
) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
