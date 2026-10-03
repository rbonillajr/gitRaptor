---
id: ADR-GRP-001
title: Stack tecnológico — motor en Rust, UI de escritorio con Tauri + React
type: adr
status: accepted
date: 2026-10-01
created: 2026-10-01
updated: 2026-10-01
deciders: [Rene Bonilla]
related: [ADR-GRP-002, ADR-GRP-003, ADR-GRP-004]
tags: [rust, tauri, react, vite, typescript, ratatui, gitoxide, rmcp, stack]
---

# ADR-GRP-001 — Stack tecnológico

## Contexto

GitRaptor (BRD-GRP-001) es una herramienta local que cada desarrollador instala en su equipo. Tiene que cumplir lo siguiente:

- Binario único para Windows, macOS y Linux (NFR-06).
- Bajo overhead: snapshots en menos de 200 ms, la UI refleja cambios en menos de 500 ms, soporta 10 o más worktrees y repos de 100K commits (NFR-04, NFR-05).
- Cero pérdida de datos (NFR-01).
- Un servidor MCP seguro (NFR-02).
- Una TUI llamativa en el MVP y una UI visual **rápida y liviana** en la Fase 3, sin el peso de Electron que tienen GitKraken y Postman (Electron + React).

## Decisión

| Componente | Tecnología |
|---|---|
| `gitraptor-core` (motor: oplog/snapshots, políticas, watcher, conflictos) | **Rust**, con **gitoxide** para leer y el **Git CLI** del sistema para escribir (respeta hooks, config y credenciales) |
| `raptor` CLI / TUI | **Rust**, con `clap` y `ratatui` |
| `raptor-mcp` | **Rust**, con `rmcp` (SDK oficial de MCP) |
| App de escritorio (Fase 3) | **Tauri** (backend Rust que llama directo al core) + **React + Vite** (SPA, sin Next.js) en el frontend; grafo en **Canvas/WebGL** con listas virtualizadas. Manejo de estado y patrones de UX según ADR-GRP-004. Componentes del design system y su UI kit (ADR-GRP-003). |
| Extensión VS Code / Cursor (Fase 3) | **TypeScript**, como cliente liviano del motor vía API local (JSON-RPC) |

Principios de rendimiento de la UI:
- El trabajo pesado (grafo, diffs, conflictos) se hace en Rust.
- El motor manda eventos incrementales (deltas).
- Se renderiza solo lo visible.
- El arranque muestra la UI primero y carga los datos de forma progresiva.

## Alternativas consideradas

- **Go** (Bubble Tea, go-git): más rápido de iterar y con una TUI muy vistosa, pero go-git está incompleto y la integración con una app de escritorio es menos natural. Queda como plan B si el spike en Rust no es viable para el equipo.
- **TypeScript/Node como motor:** obliga a distribuir un runtime y rinde peor en repos grandes. Se reserva para la extensión.
- **C#/.NET:** viable con Native AOT, pero su ecosistema de TUI y su percepción en la comunidad de developers son menores.
- **Electron + React:** el stack de GitKraken y Postman, pesado en RAM y tamaño. Se descarta.
- **UI nativa por GPU (GPUI, egui, Iced):** la máxima velocidad, pero un ecosistema inmaduro para diffs y editores complejos. Se descarta por ahora.
- **Svelte en el frontend:** algo más eficiente en actualizaciones en vivo y precedente en GitButler, pero se elige **React** porque el equipo ya lo domina (web, React Native, Next.js), tiene un ecosistema mayor y, con el grafo en Canvas y el trabajo pesado en Rust, la diferencia de rendimiento es marginal. SolidJS queda como alternativa si el cockpit en vivo lo requiere.

## Consecuencias

- ✅ Un solo lenguaje del motor a la app de escritorio, con instaladores pequeños (~5–15 MB en Tauri) y bajo consumo de RAM.
- ✅ Memory safety en un componente sensible a la seguridad (el MCP).
- ⚠️ La curva de aprendizaje de Rust hace más lento el MVP. **Mitigación:** un spike de 1 a 2 semanas antes de comprometer el roadmap.
- ✅ **Encaja con el desarrollo por agentes (BRD v0.3, D3):** el compilador y el sistema de tipos de Rust funcionan como un guardrail sobre el código que escriben los agentes. Muchos errores se detectan al compilar y no en revisión humana, lo que reduce la carga de la única persona revisora. La curva de aprendizaje humana pesa menos porque la mayor parte del código lo escriben los agentes.
- ⚠️ Tauri usa el webview de cada sistema operativo; en Linux es WebKitGTK, que es más lento. **Mitigación:** grafo en Canvas/WebGL y una matriz de pruebas en los tres sistemas.

## Validación (spike)

La decisión se confirma si el spike en Rust demuestra:
1. Snapshot y undo del working tree en menos de 200 ms en un repo mediano.
2. Predicción de conflictos entre 4 o más worktrees con `git merge-tree`.
3. Un prototipo de `rmcp` con una política que bloquee el force-push.
4. Una TUI con ratatui que muestre worktrees en vivo.

Si el spike falla en tiempo o complejidad, se reevalúa con Go (plan B) en un ADR que reemplace a este.
