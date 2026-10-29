---
mode: bulk
generated: 2026-10-28T00:00Z
updated: 2026-10-28
generator: product-owner
source: inline
total_artifacts: 18
expanded: 18
approved: 0
blocked: []
---

# User Stories — INDEX: App de escritorio

> Modo lean: cada historia es un contrato compacto para un agente (descripción, valor con BR, reglas, dependencias con lo que reutiliza y lo que falta en el motor, y Gherkin). Cada historia vive en su archivo de `user-stories/`.
>
> **Propuesta, no aprobada**: las 18 historias son `source: inline` y `draft`. Rene revisa el alcance ([context.md](./context.md), "Decisiones de alcance para Rene"); ninguna pasa a Dev Spec antes.

---

## Contexto del Feature

**Feature**: App de escritorio (BR-27, D7) · **Epic**: E-001 — MVP Fase 1
**Contexto**: [`context.md`](./context.md) (CTX-DSK-001) · **Diseño de UI-1**: [`design/design-flow.md`](./design/design-flow.md) (DSF-DSK-001) · **Benchmark**: [`research/benchmark-2026-10.md`](./research/benchmark-2026-10.md) (RES-DSK-BENCH-2026-10) · **Reglas**: las del Cockpit, la Time Machine y Guardrails (S-DSK-4) · **Enablers**: los define el Arquitecto (DEP-DSK-1 a DEP-DSK-7)

## Análisis de división

- **Flujo**: actor = desarrollador novato con agentes (Q-DSK-5). Resultado de UI-1 = poner un repo bajo GitRaptor sin terminal y ver los tres pilares: el choque antes de que pase, el "Deshacer" y el bloqueo explicado.
- **Corte**: una historia por capacidad que el usuario nota, en el mismo orden que el Cockpit; cada una reutiliza la historia del Cockpit o del pilar que ya define su comportamiento.
- **Esqueleto andante**: US-DSK-003 (conectar con el motor) → US-DSK-004 (flota en vivo). Esqueleto de escritura de UI-2: US-DSK-010 (commit).
- **Transversales (no son historias)**: la identidad de la app ante el daemon (DEP-DSK-2), la prueba de presencia humana (DEP-DSK-1), i18n y accesibilidad (DEP-DSK-7). transversal (lo define el Arquitecto).

## Índice de Historias

Nivel: **UI-1** = primera versión útil (Must); **UI-2** = sentirse como GitKraken; **UI-3** = opcionales, sin historia (ver [context.md](./context.md) § 5). "Imprescindible" = Must o Should; "opcional" = Could o Won't-now.

| ID | Título | Épica | Nivel | MoSCoW | Reutiliza | Falta | Status |
|----|--------|-------|-------|--------|-----------|-------|--------|
| [US-DSK-001](./user-stories/US-DSK-001-anadir-repo-sin-terminal.md) | El novato añade su repo a GitRaptor desde la app sin abrir una terminal | E-001 · Onboarding | UI-1 | Must | US-GRP-001, 015, 020, 022; `repo.add`, `repo.locate` | DEP-DSK-1 (comando reservado sin terminal), DEP-DSK-2 | draft |
| [US-DSK-002](./user-stories/US-DSK-002-proteger-repo-con-un-clic.md) | El novato protege su repo con un clic, sabe qué se instaló y puede quitarlo | E-001 · Onboarding | UI-1 | Must | US-GRD-001, 003, 004, 020; `guard.plan`, `guard.status`, `guard.install`, `guard.decline` | DEP-DSK-1 | draft |
| [US-DSK-003](./user-stories/US-DSK-003-estado-del-motor.md) | El usuario sabe si GitRaptor está funcionando y qué hacer cuando no | E-001 · Onboarding | UI-1 | Must | US-CKP-003, US-GRP-004, 015; `engine.snapshot`, `scope.snapshot`, `daemon.replace` | DEP-DSK-5 (arrancar el motor desde la app) | draft |
| [US-DSK-004](./user-stories/US-DSK-004-flota-en-vivo.md) | El usuario ve en vivo qué agente trabaja en cada copia de su repo y cuál pide atención | E-001 · Ver | UI-1 | Must | US-CKP-001, 002, 026, US-GRP-007, 012; `scope.subscribe`, `events.subscribe` | DEP-DSK-2 | draft |
| [US-DSK-005](./user-stories/US-DSK-005-grafo-de-carriles.md) | El usuario ve crecer la rama de cada agente como un carril sobre la rama base | E-001 · Ver | UI-1 | Must | US-CKP-022 (DS-US-CKP-022); `graph.lanes` | Lo que falte de US-CKP-022 | draft |
| [US-DSK-006](./user-stories/US-DSK-006-deshacer-de-un-boton.md) | El usuario deshace con un botón lo que hizo un agente o él mismo, y puede rehacerlo | E-001 · Proteger y deshacer | UI-1 | Must | US-CKP-021, US-TMC-002, 003, 006, 009, 012, 024; `timemachine.timeline`, `.undo`, `.redo`, `.restore`, `.notices` | DEP-DSK-1 (confirmar trabajo ajeno) | draft |
| [US-DSK-007](./user-stories/US-DSK-007-bloqueo-explicado.md) | El usuario entiende por qué GitRaptor frenó una operación y cómo hacerla a conciencia | E-001 · Proteger y deshacer | UI-1 | Must | US-CKP-019 (DS-US-CKP-019), US-GRD-005, 006, 008; `guard.log`, `audit.list`, `reserved.audit` | Nada en UI-1 (excepción por terminal, Q-DSK-9); DEP-DSK-1 si va dentro de la app | draft |
| [US-DSK-008](./user-stories/US-DSK-008-conflictos-previstos.md) | El usuario ve qué agentes van a chocar antes de que choquen y en qué archivos | E-001 · Ver | UI-1 | Must | US-CKP-006 a 009, TS-CKP-001; `prediction.list`, `prediction.pairs`, `prediction.changed` | Lo que falte de TS-CKP-001 | draft |
| [US-DSK-009](./user-stories/US-DSK-009-diff-visual.md) | El usuario revisa en la app qué cambió en una copia de trabajo o en un commit | E-001 · Cliente Git | UI-2 | Must | US-CKP-012; `diff.worktree` | DEP-DSK-3 (diff de un commit) | draft |
| [US-DSK-010](./user-stories/US-DSK-010-commit-desde-la-app.md) | El usuario guarda sus cambios en un commit desde la app y puede deshacerlo | E-001 · Cliente Git | UI-2 | Must | `commit` del catálogo (US-MCP-009, 010), US-GRD-009, 018 | DEP-DSK-4 (`commit` para el humano) | draft |
| [US-DSK-011](./user-stories/US-DSK-011-staging-por-fragmento.md) | El usuario elige qué fragmentos o líneas de un archivo entran en el commit | E-001 · Cliente Git | UI-2 | Should | `commit` con "lo preparado" | DEP-DSK-4 (preparar por fragmento y línea) | draft |
| [US-DSK-012](./user-stories/US-DSK-012-ramas-crear-cambiar-borrar.md) | El usuario crea, cambia y borra ramas desde la app sin perder trabajo | E-001 · Cliente Git | UI-2 | Must | `create-worktree`, US-GRD-008, 017, US-TMC-001 | DEP-DSK-3, DEP-DSK-4 (crear, cambiar y borrar rama) | draft |
| [US-DSK-013](./user-stories/US-DSK-013-integrar-y-poner-al-dia.md) | El usuario integra o pone al día la rama de un agente desde la app y puede deshacerlo | E-001 · Cliente Git | UI-2 | Must | US-CKP-014, 015, 016, 020, 024; `merge-into-base`, `rebase-onto-base` | DEP-DSK-1 (trabajo ajeno) | draft |
| [US-DSK-014](./user-stories/US-DSK-014-resolver-conflictos.md) | El usuario resuelve un conflicto de merge dentro de la app y termina la integración | E-001 · Cliente Git | UI-2 | Should | US-CKP-016, US-TMC-015; `abort-in-progress` | DEP-DSK-4 (marcar resuelto y continuar) | draft |
| [US-DSK-015](./user-stories/US-DSK-015-sincronizar-con-el-remoto.md) | El usuario sube y trae los cambios del remoto desde la app con la protección del repo | E-001 · Cliente Git | UI-2 | Must (si Q-DSK-14) | US-GRD-001, 008, US-TMC-014, US-GRP-012 | DEP-DSK-4 (red); **Q-DSK-14 pendiente de Rene** | draft |
| [US-DSK-016](./user-stories/US-DSK-016-worktree-para-agente.md) | El usuario prepara una copia de trabajo para un agente nuevo y descarta la de uno que terminó | E-001 · Agentes | UI-2 | Should | US-CKP-017, 018 (DS-US-CKP-018), 020; `create-worktree`, `discard-worktree` | DEP-DSK-1 (trabajo ajeno) | draft |
| [US-DSK-017](./user-stories/US-DSK-017-busqueda.md) | El usuario encuentra un commit, una rama o un archivo escribiendo lo que busca | E-001 · Cliente Git | UI-2 | Should | US-TMC-007, US-GRD-019 | DEP-DSK-3 (búsqueda) | draft |
| [US-DSK-018](./user-stories/US-DSK-018-atajos-de-teclado.md) | El usuario hace las acciones frecuentes con el teclado y las descubre en una paleta de comandos | E-001 · Cliente Git | UI-2 | Should | — | Nada en el motor | draft |

**Opcionales (UI-3, sin historia)**: PRs en GitHub y Azure DevOps (BR-19), cola de confirmación (US-GRD-015), rebase interactivo, stash, perfiles e identidades, blame, editar políticas (US-GRD-013), multi-repo, notificaciones del SO. Detalle y motivo en [context.md](./context.md) § 5.

## Orden y paralelismo (DAG)

```
                          ┌─► DSK-001 ─► DSK-002 ─────────────────────────┐
Enablers del Arquitecto   │                                              │
(DEP-DSK-2, 5, 6) ─► DSK-003 ─► DSK-004 ─┬─► DSK-005 ─┐                  ├─► Demo UI-1 (hito M2-UI, Q-DSK-7)
                                         ├─► DSK-006   ├─► DSK-008 ───────┤
                                         └─► DSK-007   ┘                  │
DEP-DSK-1 (prueba de presencia humana) ··► DSK-001, 002, 006 (sin terminal; si no, versión degradada)

UI-2 (tras la demo de UI-1 y las decisiones Q-DSK-10 y Q-DSK-14):
DSK-009 ─► DSK-010 ─► DSK-011
DSK-009 ─► DSK-013 ─► DSK-014
DSK-012 ─► DSK-015 (solo si Q-DSK-14)
DSK-004 ─► DSK-016
DSK-005 ─► DSK-017 ─► DSK-018
```

- **Paralelo en UI-1**: tras DSK-004, las ramas DSK-005, DSK-006, DSK-007 y DSK-008 son independientes. DSK-001 y DSK-002 van en paralelo con la flota, pero su versión "sin terminal" espera a DEP-DSK-1 (Q-DSK-15).
- **Paralelo en UI-2**: tres cadenas independientes (revisar y commitear, integrar, ramas y remoto).
- **Orden de UI-1 si sobra poco tiempo**: 003 → 004 → 006 → 008 → 007 → 001 → 002 → 005 (primero los diferenciales; el grafo lo tiene GitKraken).

## Comprobación de ids

Ids reservados uno a uno con `aadd-us-target.mjs --reserve` (nassa-aadd 0.13.81), escribiendo cada archivo antes de reservar el siguiente. La consulta final, sin reservar, devuelve **US-DSK-019** como siguiente id y 18 historias US-DSK conocidas. Fricción en [context.md](./context.md) § 12.
