---
id: ADR-GRP-004
title: Manejo de estado del frontend y patrones de UX (React 19)
type: adr
status: accepted
date: 2026-10-01
created: 2026-10-01
updated: 2026-10-03
deciders: [Rene Bonilla]
related: [BRD-GRP-001, ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-007]
tags: [react, state-management, tanstack-query, zustand, jotai, xstate, react-hook-form, zod, tanstack-router, ux, optimistic-ui]
---

# ADR-GRP-004 — Estado del frontend y patrones de UX

> **Alcance:** aplica a la **Fase 3** (app de escritorio Tauri + React y webviews de la extensión). En el MVP (CLI/TUI + MCP) solo aplican los **patrones de UX** de la sección 3, adaptados a la TUI (feedback inmediato, undo antes que confirmación, teclado primero, errores accionables).

## Contexto

La UI de GitRaptor (app de escritorio Tauri + React y webviews de la extensión) tiene que dar una experiencia **muy profesional**, a la altura de Linear, Raycast o Arc. Eso implica:
- respuesta instantánea;
- cero bloqueos con 10 agentes actualizándose en vivo;
- carga progresiva;
- y flujos críticos (rebase, merge, undo, acciones destructivas) sin estados imposibles ni pérdida de datos (NFR-01).

El motor en Rust (`gitraptor-core`) es la **única fuente de verdad** del estado de Git y de los agentes. Se expone por comandos de Tauri o JSON-RPC más un stream de eventos, a través de `@gitraptor/client` (ADR-GRP-002).

## Decisión

### 1. Estado por capas

| Capa | Herramienta | Responsabilidad | Ejemplos |
|---|---|---|---|
| **Estado del motor** (remoto) | **TanStack Query** | Caché, deduplicación, reintentos y estados de carga y error de los datos del motor. Los **eventos en vivo aplican deltas a la caché** (`setQueryData`), sin volver a pedir todo. | Repos, worktrees, agentes, commits, diffs, timeline, políticas |
| **Estado de UI global** | **Zustand** | Estado propio de la interfaz, con selectores finos y persistencia de preferencias. | Panel activo, selección, layout de paneles, densidad, tema, filtros |
| **Estado de alta frecuencia** | Selectores de Zustand o **Jotai** (átomos por entidad) | Aislar los re-renders: un cambio en el agente N solo repinta su tarjeta. | Actividad en vivo de cada AgentCard, contadores, indicadores de conflicto |
| **Flujos críticos** | **XState** | Máquinas de estado explícitas, testeables y visualizables, sin estados imposibles. | Asistente de rebase, merge con conflictos, undo/restore, aprobar o descartar un agente, acciones bloqueadas por política |
| **Formularios** | **React Hook Form + Zod** | Validación tipada. Los esquemas se comparten con los tipos generados del motor. | Editor de la configuración en tres niveles (`.gitraptor/settings.json`, ADR-GRP-007), crear worktree |
| **Navegación** | **TanStack Router** | Rutas y search params con tipos seguros. El estado de la vista vive en la URL, así una vista se puede restaurar o enlazar. | `/repo/:id/agents?filter=active`, `/repo/:id/timeline` |
| **Estado local** | `useState` / `useReducer` | Estado efímero de un solo componente. | Hover, un toggle, un input controlado |

**Reglas:**
- React **nunca duplica** el estado de Git. Solo lo cachea (TanStack Query) o guarda estado propio de la UI (Zustand).
- Prohibido un "store global" con datos del motor copiados.
- Cada dato tiene una sola capa dueña.

### 2. Primitivas de React 19 para la UX

| Necesidad | Primitiva | Uso en GitRaptor |
|---|---|---|
| Respuesta instantánea | `useOptimistic` (+ mutaciones optimistas de TanStack Query) | "Aprobar y hacer merge", "Descartar agente" y "Stash": la UI cambia al instante y se revierte sola si el motor falla |
| No bloquear la interacción | `useTransition` / `startTransition` | Re-filtrar o re-agrupar listas grandes sin bloquear el tipeo ni los clics |
| Búsquedas y filtros suaves | `useDeferredValue` | Buscador de commits (50K o más) sin lag ni parpadeo |
| Carga progresiva | `Suspense` + skeletons por panel | El cockpit aparece de inmediato; el diff y el timeline cargan de forma independiente |
| Estado de acciones | `useActionState` | Pending, éxito y error de acciones (undo, restore, push) sin código repetido |
| Fallos contenidos | Error Boundaries por panel | Si falla un panel, el resto de la app sigue funcionando y se ofrece "Reintentar" |

### 3. Patrones de UX obligatorios

| Patrón | Regla |
|---|---|
| **Feedback < 100 ms** | Toda acción del usuario produce una respuesta visual en menos de 100 ms (optimista, pending o skeleton). |
| **Undo antes que confirmación** | Las acciones reversibles se ejecutan de inmediato y muestran un toast con "Deshacer" (respaldado por la Time Machine). |
| **Confirmación solo para lo irreversible** | Las acciones irreversibles o de alto riesgo usan `ConfirmDialog` (ADR-GRP-003), que explica qué va a pasar y qué se pierde. |
| **Teclado primero** | Command palette (Ctrl/Cmd + K), atajos documentados, foco gestionado y navegación completa sin mouse. |
| **El estado sobrevive** | Al reabrir la app se restauran el panel, los filtros, la selección y el layout (Zustand persistido + URL). |
| **Re-renders mínimos** | Selectores finos y `memo` en los componentes que reciben eventos en vivo. Presupuesto: 60 fps con 10 agentes activos. |
| **Errores accionables** | Los mensajes dicen qué pasó y qué hacer ("Rebase detenido por conflicto en `auth.ts` → Resolver / Abortar"), nunca solo "Error". |
| **Vacíos útiles** | Cada empty state indica la siguiente acción ("No hay agentes activos → Crear worktree para un agente"). |

## Alternativas consideradas

- **Redux Toolkit (+ RTK Query):** robusto, pero con más ceremonia. TanStack Query + Zustand cubren lo mismo con menos código y re-renders más finos.
- **Solo Context API:** provoca re-renders amplios con datos en vivo. Se descarta para el estado compartido.
- **MobX / Valtio:** reactividad fina, pero un modelo menos explícito. El equipo prefiere stores y máquinas de estado declarativas.
- **Sin XState (solo reducers):** viable para flujos simples, pero los flujos de Git con conflictos tienen muchos estados intermedios. Las máquinas explícitas reducen el riesgo de pérdida de datos.

## Consecuencias

- ✅ Una separación clara de responsabilidades: cada dato tiene una sola capa dueña.
- ✅ UX fluida con 10 o más agentes en vivo gracias a deltas en la caché y re-renders aislados.
- ✅ Los flujos críticos son testeables como máquinas de estado, lo que refuerza NFR-01.
- ⚠️ Son varias librerías. **Mitigación:** una guía de "qué capa uso" en el README del frontend, ejemplos de referencia y revisión en el code review.
- ⚠️ Las actualizaciones optimistas pueden mostrar estados que luego se revierten. **Mitigación:** se usan solo en operaciones de alta probabilidad de éxito y siempre con rollback visible.

## Referencias

- [BRD-GRP-001 — Documento de negocio de GitRaptor](../../business/gitraptor-documento-de-negocio.md)

---

Enmienda 2026-10-03: referencias a policy.yaml sustituidas por ADR-GRP-007 (configuración en tres niveles).
