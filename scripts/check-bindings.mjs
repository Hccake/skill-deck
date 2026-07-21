import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const CANONICAL_BINDINGS = join(ROOT_DIR, 'src', 'bindings.ts');
const tempDir = mkdtempSync(join(tmpdir(), 'skill-deck-bindings-'));
const generatedBindings = join(tempDir, 'bindings.ts');

function run(command, args, env = process.env) {
  const result = spawnSync(command, args, {
    cwd: ROOT_DIR,
    env,
    stdio: 'inherit',
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

try {
  run(
    'cargo',
    [
      'test',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      'command_surface_tests::export_bindings',
      '--',
      '--ignored',
      '--exact',
    ],
    { ...process.env, SKILL_DECK_BINDINGS_OUT: generatedBindings }
  );
  const expected = readFileSync(CANONICAL_BINDINGS);
  const actual = readFileSync(generatedBindings);
  if (!expected.equals(actual)) {
    throw new Error(
      'Generated bindings differ from src/bindings.ts. Run pnpm bindings:generate.'
    );
  }

  console.log('Bindings are up to date.');
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  rmSync(tempDir, { recursive: true, force: true });
}
