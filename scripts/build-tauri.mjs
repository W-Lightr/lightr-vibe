#!/usr/bin/env node
/**
 * 跨平台打包入口：把参数原样转给 `tauri build`。
 *
 * pnpm 会把 `pnpm build:tauri -- --bundles nsis` 里的 `--` 原样传入，
 * 先去掉它，避免 Tauri CLI 把它当成位置参数。
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const rawExtra = process.argv.slice(2);
const extra = rawExtra[0] === '--' ? rawExtra.slice(1) : rawExtra;
const tauriArgs = ['tauri', 'build', ...extra];

const result =
  process.platform === 'win32'
    ? spawnSync('cmd.exe', ['/d', '/s', '/c', ['pnpm', ...tauriArgs].join(' ')], {
        cwd: projectRoot,
        stdio: 'inherit',
      })
    : spawnSync('pnpm', tauriArgs, { cwd: projectRoot, stdio: 'inherit' });

if (result.error) {
  console.error(`[build] 无法启动 Tauri CLI: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
