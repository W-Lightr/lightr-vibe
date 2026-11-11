#!/usr/bin/env node
/**
 * 跨平台打包入口：按当前平台选择 bundle.resources 配置。
 *
 * 这些配置不能命名为 `tauri.<platform>.conf.json`，否则 Tauri 会在 `tauri dev`
 * 时也自动合并，导致尚未暂存 lightr-ask-mcp 的全新工作区启动失败。
 * 仅在 build 时通过 --config 传入。
 *
 * 额外参数原样透传，例如：`pnpm build:tauri -- --bundles nsis`。
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const platform =
  process.platform === 'win32'
    ? 'windows'
    : process.platform === 'darwin'
      ? 'macos'
      : 'linux';

const bundleConfig = path.join('src-tauri', `tauri.bundle.${platform}.conf.json`);
// 透传参数放在 --config 之前：部分 Tauri CLI 版本会把 --config 之后的
// 选项误当成 runner(cargo) 参数；同时保留用户显式 `--` 之后的内容。
const rawExtra = process.argv.slice(2);
// pnpm 会把 `pnpm build:tauri -- --bundles nsis` 里的 `--` 原样传入，先去掉它。
const extra = rawExtra[0] === '--' ? rawExtra.slice(1) : rawExtra;
const separator = extra.indexOf('--');
const beforeSeparator = separator === -1 ? extra : extra.slice(0, separator);
const afterSeparator = separator === -1 ? [] : extra.slice(separator);
const tauriArgs = [
  'tauri',
  'build',
  ...beforeSeparator,
  '--config',
  bundleConfig,
  ...afterSeparator,
];

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
