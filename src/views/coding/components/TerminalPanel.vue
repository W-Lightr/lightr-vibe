<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'

const el = ref<HTMLElement | null>(null)

let term: Terminal | null = null
let fitAddon: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null

onMounted(() => {
  if (!el.value) return
  term = new Terminal({
    fontSize: 12.5,
    fontFamily: 'Consolas, "Courier New", monospace',
    cursorBlink: true,
    theme: { background: '#171717', foreground: '#e5e5e5' },
  })
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(el.value)
  fitAddon.fit()

  term.writeln('Lightr-vibe 终端占位 — 后续通过 tauri-plugin-shell sidecar 接入真实 shell')

  // 本地回显（真实 PTY 接入前的占位行为）
  term.onData((data) => {
    term?.write(data)
  })

  resizeObserver = new ResizeObserver(() => fitAddon?.fit())
  resizeObserver.observe(el.value)
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  term?.dispose()
  term = null
})
</script>

<template>
  <div ref="el" class="h-full w-full bg-neutral-900 p-1" />
</template>
