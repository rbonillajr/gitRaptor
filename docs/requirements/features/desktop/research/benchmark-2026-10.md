---
id: RES-DSK-BENCH-2026-10
title: "Benchmark de GUIs de Git para la app de escritorio: GitKraken, GitHub Desktop, Fork y las apps de agentes"
type: research
status: proposed
version: 0.1
date: 2026-10-28
author: Rene Bonilla (orquestado por agente)
domain: GRP
related:
  business: [BRD-GRP-001]
  research: [RES-GRP-COMP-2026-10]
  features: [desktop, cockpit, time-machine, guardrails]
tags: [competencia, gitkraken, github-desktop, fork, gitbutler, tower, novatos, escritorio, benchmark]
changelog:
  - 0.1 (2026-10-28): Primera versión. Investigación web con un subagente `internet-researcher` (sonnet) para la decisión D7 (app de escritorio en el MVP).
---

# Benchmark de GUIs de Git para la app de escritorio

> **Qué es este documento:** el benchmark que alimenta la feature [desktop](../context.md) y la decisión D7 del [documento de negocio](../../../../business/gitraptor-documento-de-negocio.md). Complementa, sin sustituirla, la [investigación competitiva de 2026-10](../../../../business/research/competitive-2026-10.md), que ya analizó la vista de agentes de GitKraken 12 y GitLens 19.
>
> **Método:** investigación web del 2026-10-28. Cada dato lleva su fuente en el § 6 (`[n]`). **NV** = no verificado: la fuente no lo dice o no se pudo confirmar. Muchas comparativas son de competidores (GitKraken, Gitoryx, GitTree, Nimbalyst) y los precios discrepan entre fuentes; se marcan así. Los nombres de productos de terceros se usan solo para identificarlos; GitRaptor no está afiliado a ninguno.

## 1. Resumen

- **GitKraken Desktop ya cubre "agentes + worktrees + grafo"** desde la 12.0 (16-abr-2026): tarjetas de sesión con estado, worktrees en el grafo y creación del worktree con el agente en un paso [1].
- **GitHub Desktop es la referencia de simplicidad** para novatos: lenguaje llano y pocas decisiones, pero sin grafo y con un undo limitado a "Undo commit" [8].
- **GitButler es lo más cercano a snapshots del trabajo sin commitear** (oplog con undo y redo), pero solo de lo que se hace con `but` [11].
- **Ninguna herramienta encontrada combina** la predicción de conflictos entre worktrees, los snapshots con undo universal y los guardrails por repo en una sola app. **Ese es el hueco de GitRaptor** (§ 4).

## 2. Tabla comparativa

| Capacidad | GitKraken | GitHub Desktop | Fork | Tower | GitButler | Apps de agentes (Conductor, Nimbalyst, Vibe Kanban, Claude Squad, Warp) |
|---|---|---|---|---|---|---|
| Grafo de commits | Sí [1][2] | No: lista plana, una rama a la vez [8] | Sí (NV) | Sí (NV) | Vista propia por ramas (NV) | Mayormente no; Warp usa un panel de diff [14] |
| Ramas, PRs e integraciones | GitHub, GitLab y Azure DevOps; Launchpad; Jira y Trello en Pro [2] | Centrado en GitHub [8] | PRs solo de GitHub [5] | Sí (NV en detalle) | NV | Conductor: flujo de diff y PR con Linear y GitHub [10] |
| Diff | Sí, con blame e historial del archivo [2] | Sí | Sí | Sí | Sí | Warp: panel de revisión con comentarios inline al agente [14] |
| Staging por hunk o línea | La página no lo menciona [2] | Parcial por líneas (fuentes contradictorias) [8] | Por línea [5] | Por línea [6] | NV | Warp: revertir hunks [14] |
| Merge y conflictos visuales | Merge tool con autorresolución por IA [2] | Lista de archivos en conflicto; se edita en un editor externo [8][9] | Resolvedor integrado [5] | Asistente (según un competidor, por archivo completo) [6] | NV | Normalmente delegado |
| Prevención de conflictos | Conflict Prevention: avisa de solapes con miembros del equipo o la rama destino [3] | No | No | No | No (evita conflictos asignando cambios a ramas) [12] | Ninguna de forma nativa [13] |
| Undo | Cmd/Ctrl+Z, solo la última acción: checkout, commit, discard, borrar rama, reset, rebase [4] | "Undo commit" (no tras push) y Revert [8] | Reflog [5] | Cmd/Ctrl+Z para commits, merges y stash [6] | `but undo`/`redo` y oplog con restauración [11] | No |
| Onboarding para novatos | Tutoriales (NV) | Muy simple, asume cero conocimiento [8] | "Friendly" (marketing) [5] | NV | NV | Bajo |
| Perfiles | Sí, en Pro [2] | No | No | NV | NV | No |
| Worktrees | El grafo muestra los worktrees activos [1] | NV | NV | Sí [6] | Alternativa a los worktrees [12] | Sí, uno por agente [10] |
| Agentes en vivo | Sesiones de Claude Code, Codex, Copilot CLI, Gemini CLI y OpenCode con estado [1] | No | No | No | Pestaña de Claude retirada; ahora skill y plugin [12] | Núcleo del producto [10][14] |
| Políticas o guardrails | No encontrado | No | No | No | No | No encontrado |
| Snapshots del trabajo sin commitear | No documentado [4] | No | No | No | Sí, el oplog incluye lo no commiteado [11] | No |
| Plataformas | macOS, Windows y Linux [2] | macOS y Windows | macOS y Windows [5] | macOS y Windows [6] | NV | Conductor solo macOS; Nimbalyst multiplataforma [10] |
| Precio y licencia | Gratis en repos locales y públicos; Pro de pago ($4 o $4,95 por usuario y mes según la fuente) [2][3] | Gratis (NV) | Pago único, $49,99 o $59,99 según la fuente [5] | $69 o $129 al año según la fuente [6] | NV | Conductor Pro $50/mes; Nimbalyst MIT; Vibe Kanban Apache-2.0, con la nube cerrada en abr-2026 [10] |

Otras dos, de pasada: **Sourcetree** es gratis, para macOS y Windows, exige cuenta de Atlassian y se actualiza poco; **Sublime Merge** cuesta $99 en pago único, es rápido y tiene un undo limitado [7].

### 2.1 GitKraken y los agentes, en detalle

- **Vista de sesiones:** tarjetas con estado (corriendo, esperando input, terminado). Crear una sesión genera el worktree, corre el setup y lanza el agente [1].
- **Grafo:** incluye cada worktree activo [1].
- **Versiones:** Cursor CLI llega en la 12.5; la 12.4 permite iniciar una sesión desde un issue de Launchpad [1].
- **Incidente de confianza:** la 12.0 instaló hooks de Claude Code sin avisar y rompió configuraciones; la 12.0.1 (18-abr) lo corrigió y la 12.6 dejó de instalar plugins automáticamente [1].
- **Conflictos entre agentes:** la página de Agent Mode no describe ninguna detección de conflictos entre sesiones [1]. Conflict Prevention compara con el equipo o la rama destino, no entre worktrees locales [3]. En qué plan entran Agent Mode y Conflict Prevention (Pro o Advanced) es NV: las páginas de ayuda se contradicen [1][3].
- **IA adicional:** mensajes de commit, Commit Composer, descripciones de PR y autorresolución de conflictos [2].

## 3. Patrones de UX para novatos que conviene copiar

| # | Patrón | De dónde | Cómo lo usa GitRaptor |
|---|---|---|---|
| P1 | Una acción de deshacer visible, con Cmd/Ctrl+Z y rehacer | GitKraken [4], Tower [6] | "Deshacer" de un botón en el historial, pero sobre la Time Machine: cubre más que la última acción y también el trabajo sin commitear |
| P2 | Cada restauración es una entrada nueva, así que también se deshace | GitButler [11] | Ya es la regla de la Time Machine (el undo es reversible) |
| P3 | Lenguaje no técnico y pocas decisiones | GitHub Desktop [8] | Copy en lenguaje llano con glosario en tooltip (ver [design-flow](../design-flow.md)) |
| P4 | Tarjetas de sesión con estado y acciones de un clic | GitKraken [1] | Flota en vivo (US-CKP-001) como tarjetas |
| P5 | Crear el worktree en un paso: nombre, agente e "Iniciar", sin decir "worktree" | GitKraken [1] | UI-2 (US-CKP-018 desde la GUI) |
| P6 | Comentarios de revisión inline que llegan al agente | Warp [14] (el detalle viene de un post de DEV, no de su documentación) | Fuera de UI-1; candidato tras BR-17 (review queue) |
| P7 | Avisos proactivos de solape con acciones (push, rebase, merge) | GitKraken Conflict Prevention [3] | Conflictos previstos entre carriles (US-CKP-006/007/008) |
| P8 | Lista de archivos en conflicto con estado y "Continuar merge" | GitHub Desktop [9] | UI-2, resolviendo dentro de la app |
| P9 | Ajustes por repositorio | GitKraken [3] | Guardrails ya es por repo |
| P10 | **No instalar nada sin permiso**, y poder revertirlo | Lección del incidente de hooks de GitKraken [1] | Proteger un repo instala hooks: la GUI lo pide y lo explica, y "Desproteger" lo revierte |

## 4. Qué no cubre ninguno para quien trabaja con agentes: el hueco de GitRaptor

1. **Predicción de conflictos entre worktrees de agentes antes de que ocurran.** Conflict Prevention de GitKraken compara con el equipo o la rama destino, no entre worktrees locales [3]. Un análisis de Upsun dice que ninguna herramienta avisa cuando dos agentes pueden editar el mismo código, y el roundup de Augment indica que la resolución automática entre agentes "mostly doesn't" existir; solo hay scripts caseros y prototipos [13]. → **Conflictos previstos** (TS-CKP-001, `prediction.*`).
2. **Snapshots automáticos y undo universal del trabajo sin commitear y de Git crudo.** El undo de GitKraken es de una acción y su documentación no dice si cubre push, stash o lo no commiteado [4]. GitButler cubre lo no commiteado, pero solo de las operaciones hechas con `but` [11]; los comandos de Git crudo que ejecuta un agente (`reset --hard`, `checkout`, `clean`) no pasan por su oplog (inferencia, NV). → **Time Machine** (`tm.*`).
3. **Guardrails por repo con excepción consciente del humano.** No se encontró ninguna GUI con políticas por repo que bloqueen el force-push u otras operaciones peligrosas; Conflict Prevention tiene un control de administrador solo en el plan Business [3]. Es evidencia negativa y no se pudo comprobar por completo. → **Guardrails** (`guard.*`).
4. **Todo junto en una app pensada para novatos.** GitKraken tiene agentes y grafo, pero en plan de pago y con el incidente de los hooks [1]; las apps de agentes ofrecen worktrees y revisión, pero no seguridad de Git [10][13][14]; GitHub Desktop es simple y no sabe nada de agentes [8].

**Conclusión para el alcance.** "Ver la flota" no diferencia (ya lo concluyó [RES-GRP-COMP-2026-10](../../../../business/research/competitive-2026-10.md)). Lo que diferencia la app es **mostrar a un novato, sin terminal, los tres pilares que nadie más tiene**: el conflicto antes de que pase, el "Deshacer" que lo cubre todo y el bloqueo explicado. Por eso UI-1 los prioriza, y lo que hace que "se sienta como GitKraken" (diff, staging, merge visual, PRs) queda para UI-2 (ver [context.md](../context.md)).

## 5. Cautelas

- NV: el grafo, el onboarding y los perfiles de Fork, Tower y GitButler, más allá de lo citado; los precios de GitHub Desktop y GitButler; el soporte de worktrees de Fork y GitHub Desktop.
- Sculptor: las fuentes discrepan entre contenedor Docker y worktree [10].

## 6. Fuentes

Todas consultadas el 2026-10-28.

1. GitKraken Desktop 12.0, Agent Mode: https://gitkraken.com/blog/youre-running-agents-your-tooling-is-still-catching-up · notas de versión: https://help.gitkraken.com/gitkraken-desktop/current/ · 12.0.1 (hooks): https://gitkraken.com/blog/gitkraken-desktop-12-0-1-update
2. GitKraken Desktop, página de producto: https://gitkraken.com/git-client
3. Conflict Prevention: https://help.gitkraken.com/gitkraken-desktop/conflict-prevention/ · precios (Capterra): https://www.capterra.com/p/10035742/GitKraken-Desktop/pricing/
4. Undo y redo en GitKraken: https://help.gitkraken.com/gitkraken-desktop/undo-and-redo/
5. Fork: https://git-fork.com/ · precio: https://toolradar.com/tools/fork-git
6. Tower, comparativa propia: https://www.git-tower.com/blog/best-git-client · comparativa de un competidor: https://gittree.app/en/tower-alternative
7. Sourcetree y Sublime Merge: https://toolradar.com/tools/sourcetree · https://thesoftwarescout.com/best-git-clients-2026-top-gui-tools-for-version-control/
8. GitHub Desktop, gestión de commits: https://docs.github.com/en/desktop/managing-commits · comparativa de un competidor: https://gitkraken.com/compare/gitkraken-vs-github-desktop
9. Sincronizar la rama en GitHub Desktop: https://docs.github.com/en/enterprise-server@3.17/desktop/working-with-your-remote-repository-on-github-or-github-enterprise/syncing-your-branch-in-github-desktop
10. Roundups de apps de agentes: https://nimbalyst.com/blog/best-agent-management-tools-2026/ · https://dev.to/stravukarl/best-tools-for-managing-parallel-ai-coding-agents-in-2026-14l8
11. GitButler, Operations Log: https://docs.gitbutler.com/cli-guides/cli-tutorial/operations-log · https://docs.gitbutler.com/commands/but-undo
12. GitButler, pestaña de agentes: https://blog.gitbutler.com/agents-tab · Trigger.dev (abr-2026): https://trigger.dev/blog/parallel-agents-gitbutler
13. Predicción de conflictos entre agentes: https://developer.upsun.com/posts/ai/git-worktrees-for-parallel-ai-coding-agents · https://www.augmentcode.com/tools/open-source-agent-orchestrators · https://codeongrass.com/blog/parallel-worktrees-conflict-prediction/
14. Warp, revisión de código y worktrees: https://docs.warp.dev/code/code-review/ · https://docs.warp.dev/code/git-worktrees/
