# Frontend Development Guidelines

> Best practices for frontend development in this project.

---

## Overview

This directory documents the actual conventions of the Lightr-vibe frontend
(Vue 3 `<script setup>` + TypeScript strict + Pinia setup stores + Tailwind v4
+ naive-ui, inside a Tauri 2 shell). All examples are taken from real files in
`src/`.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module organization and file layout | Filled |
| [Component Guidelines](./component-guidelines.md) | Component patterns, props, composition | Filled |
| [Hook Guidelines](./hook-guidelines.md) | Composables (`useXxx`), async/cancellation patterns | Filled |
| [State Management](./state-management.md) | Pinia setup stores, boundaries, IDs | Filled |
| [Quality Guidelines](./quality-guidelines.md) | Code standards, verification commands, forbidden patterns | Filled |
| [Type Safety](./type-safety.md) | Type patterns, nullability conventions | Filled |
| [UI Style](./ui-style.md) | Visual design language: color, type, shape, motion | Filled |
| [Lazy Loading](./lazy-loading.md) | Route-level lazy entries, async heavy dependencies, prefetch | Filled |
| [Tauri Bridge](./tauri-bridge.md) | Rust command contracts, dual-channel services, JSONC safe-write | Filled |

---

## How to Fill These Guidelines

For each guideline file:

1. Document your project's **actual conventions** (not ideals)
2. Include **code examples** from your codebase
3. List **forbidden patterns** and why
4. Add **common mistakes** your team has made

The goal is to help AI assistants and new team members understand how YOUR project works.

---

**Language**: All documentation should be written in **English**.
