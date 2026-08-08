import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import GithubSlugger from "github-slugger";
import { fromMarkdown } from "mdast-util-from-markdown";

const EXCLUDED_DOC_DIRECTORIES = new Set(["plans", "superpowers"]);

function nodeText(node) {
  if (typeof node.value === "string") return node.value;
  return Array.isArray(node.children) ? node.children.map(nodeText).join("") : "";
}

function walk(node, visit) {
  visit(node);
  if (Array.isArray(node.children)) {
    for (const child of node.children) walk(child, visit);
  }
}

async function markdownFilesUnder(directory, relativeDirectory = "") {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const relativePath = path.join(relativeDirectory, entry.name);
    if (entry.isDirectory()) {
      if (relativeDirectory === "docs" && EXCLUDED_DOC_DIRECTORIES.has(entry.name)) continue;
      files.push(...await markdownFilesUnder(path.join(directory, entry.name), relativePath));
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      files.push(relativePath);
    }
  }
  return files;
}

export async function documentationEntries(root) {
  const rootEntries = await readdir(root, { withFileTypes: true });
  const files = rootEntries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
    .map((entry) => entry.name);
  const docs = rootEntries.find((entry) => entry.isDirectory() && entry.name === "docs");
  if (docs) files.push(...await markdownFilesUnder(path.join(root, "docs"), "docs"));
  return files.sort();
}

function externalLink(url) {
  return /^[a-z][a-z+.-]*:/i.test(url) || url.startsWith("//");
}

function localTarget(root, sourcePath, url) {
  const hashIndex = url.indexOf("#");
  const rawPath = hashIndex >= 0 ? url.slice(0, hashIndex) : url;
  const rawFragment = hashIndex >= 0 ? url.slice(hashIndex + 1) : "";
  const withoutQuery = rawPath.split("?", 1)[0];
  const decodedPath = decodeURIComponent(withoutQuery);
  const targetPath = decodedPath.length === 0
    ? sourcePath
    : decodedPath.startsWith("/")
      ? decodedPath.slice(1)
      : path.join(path.dirname(sourcePath), decodedPath);
  return {
    path: path.normalize(targetPath),
    fragment: decodeURIComponent(rawFragment),
  };
}

async function parsedDocument(root, relativePath, cache) {
  if (cache.has(relativePath)) return cache.get(relativePath);
  const content = await readFile(path.join(root, relativePath), "utf8");
  const tree = fromMarkdown(content);
  const slugger = new GithubSlugger();
  const anchors = new Set();
  walk(tree, (node) => {
    if (node.type === "heading") anchors.add(slugger.slug(nodeText(node)));
  });
  const parsed = { tree, anchors };
  cache.set(relativePath, parsed);
  return parsed;
}

export async function checkDocumentation(root, entries = undefined) {
  const sourcePaths = entries ?? await documentationEntries(root);
  const cache = new Map();
  const problems = [];

  for (const sourcePath of sourcePaths) {
    const { tree } = await parsedDocument(root, sourcePath, cache);
    const links = [];
    walk(tree, (node) => {
      if ((node.type === "link" || node.type === "image") && !externalLink(node.url)) {
        links.push(node);
      }
    });

    for (const link of links) {
      const target = localTarget(root, sourcePath, link.url);
      const line = link.position?.start.line ?? 1;
      let targetStat;
      try {
        targetStat = await stat(path.join(root, target.path));
      } catch {
        problems.push(`${sourcePath}:${line}: missing file: ${target.path}`);
        continue;
      }
      if (!target.fragment || !targetStat.isFile() || !target.path.endsWith(".md")) continue;
      const { anchors } = await parsedDocument(root, target.path, cache);
      if (!anchors.has(target.fragment)) {
        problems.push(`${sourcePath}:${line}: missing anchor: ${target.path}#${target.fragment}`);
      }
    }
  }
  return problems;
}

async function main() {
  const root = process.cwd();
  const entries = await documentationEntries(root);
  const problems = await checkDocumentation(root, entries);
  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    process.exitCode = 1;
    return;
  }
  console.log(`Checked ${entries.length} Markdown files.`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
