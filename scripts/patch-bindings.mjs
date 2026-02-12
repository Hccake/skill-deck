/**
 * tauri-specta 生成的 bindings.ts 包含未使用的 TAURI_CHANNEL 和 __makeEvents__，
 * 会触发 tsconfig 的 noUnusedLocals 报错。此脚本在生成后自动修补。
 *
 * 用法: node scripts/patch-bindings.mjs
 */
import { readFileSync, writeFileSync } from 'fs';

const BINDINGS_PATH = 'src/bindings.ts';

let content = readFileSync(BINDINGS_PATH, 'utf-8');
let patched = false;

// 1. 移除未使用的 TAURI_CHANNEL import
if (content.includes('Channel as TAURI_CHANNEL')) {
  content = content.replace(
    /import\s*\{\s*invoke\s+as\s+TAURI_INVOKE\s*,\s*Channel\s+as\s+TAURI_CHANNEL\s*,?\s*\}\s*from\s*"@tauri-apps\/api\/core"\s*;/,
    'import {\n\tinvoke as TAURI_INVOKE,\n} from "@tauri-apps/api/core";'
  );
  patched = true;
}

// 2. 导出未使用的 __makeEvents__ 以避免 noUnusedLocals 报错
if (content.includes('function __makeEvents__') && !content.includes('export function __makeEvents__')) {
  content = content.replace(
    'function __makeEvents__',
    'export function __makeEvents__'
  );
  patched = true;
}

if (patched) {
  writeFileSync(BINDINGS_PATH, content, 'utf-8');
  console.log('✓ bindings.ts patched');
} else {
  console.log('✓ bindings.ts already clean');
}
