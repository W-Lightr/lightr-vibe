#!/usr/bin/env node
/**
 * Tauri `beforeBuildCommand` 的跨平台实现（Windows / macOS / Linux 通用）。
 *
 * 依次完成：
 *   1. `pnpm build`：生成前端产物（Tauri `frontendDist` 指向 ../dist）。
 *   2. `cargo build --release --bin lightr-ask-mcp`：编译伴生 MCP 进程；
 *      CI 交叉编译时由 LIGHTR_TARGET_TRIPLE 指定目标三元组。
 *   3. Windows 本地构建时运行 smoke 测试，确认伴生进程可正常响应。
 *
 * 伴生进程是同一个 Cargo 包里的第二个 `[[bin]]`，Tauri CLI 会自动把它作为
 * external binary 打进安装包（Windows 安装目录 / macOS Contents/MacOS /
 * Linux usr/bin），运行时由 `ask_bin_path` 优先在主程序同目录查找。
 *
 * 因此这里不再复制到 `src-tauri/binaries`，也不使用 bundle.resources：
 * tauri-build 会在编译期校验 resources 路径，而本脚本的 cargo build 发生在
 * 伴生进程生成之前，干净工作区会直接构建失败。
 */

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const tauriDir = path.join(projectRoot, 'src-tauri');

function run(command, args, { cwd = projectRoot } = {}) {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.error) {
    console.error(`[prepare] 无法启动 ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

/** 用平台 shell 执行一行命令（Windows 的 pnpm 是 .cmd，需要 cmd.exe 承载）。 */
function runLine(line, cwd = projectRoot) {
  if (process.platform === 'win32') {
    run('cmd.exe', ['/d', '/s', '/c', line], { cwd });
    return;
  }
  run('sh', ['-c', line], { cwd });
}

runLine('pnpm build');

// CI 交叉编译（如 macOS 构建另一个架构）必须把 ask-mcp 编到同一目标架构。
// 优先用 workflow 显式传入的 LIGHTR_TARGET_TRIPLE；Tauri 也会设置
// TAURI_ENV_TARGET_TRIPLE，但原生构建时它等于宿主三元组，此时必须保持
// cargo 默认的 target/release 目录（否则 Windows smoke 测试找不到产物）。
function hostTriple() {
  const result = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
  if (result.status !== 0 || !result.stdout) return '';
  const match = result.stdout.match(/^host:\s*(.+)$/m);
  return match ? match[1].trim() : '';
}

let triple = (process.env.LIGHTR_TARGET_TRIPLE || '').trim();
if (!triple) {
  const tauriTriple = (process.env.TAURI_ENV_TARGET_TRIPLE || '').trim();
  if (tauriTriple && tauriTriple !== hostTriple()) {
    triple = tauriTriple;
  }
}
const isWindows = triple ? triple.includes('windows') : process.platform === 'win32';
const exeSuffix = isWindows ? '.exe' : '';

const cargoArgs = ['build', '--release', '--bin', 'lightr-ask-mcp'];
if (triple) {
  cargoArgs.push('--target', triple);
}
run('cargo', cargoArgs, { cwd: tauriDir });

const releaseDir = path.join(tauriDir, 'target', ...(triple ? [triple] : []), 'release');
const builtBinary = path.join(releaseDir, `lightr-ask-mcp${exeSuffix}`);
if (!existsSync(builtBinary)) {
  console.error(`[prepare] 未找到伴生 MCP 构建产物: ${builtBinary}`);
  process.exit(1);
}
console.log(`[prepare] 伴生 MCP 产物: ${path.relative(projectRoot, builtBinary)}`);

if (process.platform === 'win32' && !triple) {
  run('powershell.exe', [
    '-NoProfile',
    '-ExecutionPolicy',
    'Bypass',
    '-File',
    path.join(projectRoot, 'scripts', 'smoke-lightr-ask-mcp.ps1'),
    '-Configuration',
    'release',
  ]);
}
