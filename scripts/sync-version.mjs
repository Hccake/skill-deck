/**
 * 从 package.json 读取版本号，同步更新 src-tauri/Cargo.toml。
 * 保持 package.json 为版本号的唯一来源（Single Source of Truth）。
 *
 * 用法: node scripts/sync-version.mjs
 */
import { readFileSync, writeFileSync } from 'fs';

const pkg = JSON.parse(readFileSync('package.json', 'utf-8'));
const version = pkg.version;

const CARGO_PATH = 'src-tauri/Cargo.toml';
let cargo = readFileSync(CARGO_PATH, 'utf-8');

// 替换 [package] 下的 version 字段（只替换第一个匹配，即 package 自身的版本）
const updated = cargo.replace(
  /^(version\s*=\s*)"[^"]*"/m,
  `$1"${version}"`
);

if (updated !== cargo) {
  writeFileSync(CARGO_PATH, updated, 'utf-8');
  console.log(`✓ Cargo.toml version synced to ${version}`);
} else {
  console.log(`✓ Cargo.toml version already ${version}`);
}
