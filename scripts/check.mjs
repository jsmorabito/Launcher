#!/usr/bin/env node
/**
 * Run launcher doctor and type checks across the project.
 * Cross-platform (Node.js, no bash dependencies).
 */
import { execSync } from 'child_process';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

let hasErrors = false;

// Run launcher doctor from the SDK directory
console.log('Running launcher doctor...\n');
try {
  execSync('node dist/cli/index.js doctor', {
    cwd: resolve(root, 'launcher-sdk'),
    stdio: 'inherit',
  });
} catch {
  hasErrors = true;
}

// Run the design-system checker
console.log('\nChecking design system compliance...\n');
try {
  execSync('node scripts/check-design.mjs', { cwd: root, stdio: 'inherit' });
} catch {
  hasErrors = true;
}

// Run svelte-check in the launcher app
console.log('\nRunning type checks (svelte-check)...\n');
try {
  execSync('pnpm run check', {
    cwd: resolve(root, 'launcher-app'),
    stdio: 'inherit',
  });
  console.log('\n✓ Type checks passed');
} catch {
  console.error('\n✗ Type checks failed');
  hasErrors = true;
}

if (hasErrors) {
  process.exit(1);
}
