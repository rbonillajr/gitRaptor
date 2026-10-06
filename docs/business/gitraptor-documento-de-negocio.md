---
id: BRD-GRP-001
title: GitRaptor — Documento de Negocio
type: business-requirements
status: draft
version: 0.8
date: 2026-10-06
author: Rene Bonilla
tags: [git, ai-agents, worktrees, mcp, tui, cli, agent-cockpit, safety-net, guardrails, azure-devops, brd]
changelog:
  - 0.1 (2026-10-01): Extensión VS Code/Cursor estilo GitKraken.
  - 0.2 (2026-10-01): Pivote a "Git para la era de los agentes de IA": motor local con CLI/TUI + servidor MCP; la extensión y la app de escritorio pasan a ser capas visuales posteriores.
  - 0.3 (2026-10-01): Decisiones cerradas: herramienta interna al inicio, Claude Code + Cursor como agentes del MVP, desarrollo por una persona orquestando múltiples agentes de IA (dogfooding desde el día uno).
  - 0.4 (2026-10-02): BR-11 deja de fijar el archivo `.gitraptor/policy.yaml`; el formato y la estructura de la configuración del repo se deciden en un ADR (propuesta: JSON en tres niveles perfil/repo/local, con secciones `permissions` y `policies`).
  - 0.5 (2026-10-03): D2 cambia: el MVP da soporte completo solo a Claude Code; después Codex y luego Cursor, uno por uno. Mientras tanto, los demás agentes se aceptan como "otro agente" mediante registro explícito.
  - 0.6 (2026-10-05): D4 y D5 (decisiones de Rene Bonilla): modelo **open core**, con el núcleo gratuito bajo FSL-1.1-ALv2 y una edición de equipo comercial (BR-23 y BR-25); nombre `gitraptor` en los canales y comando `raptor`. Se cierra la pregunta abierta 2.
  - 0.7 (2026-10-05): Research de mercado actualizado ([RES-GRP-COMP-2026-10](research/competitive-2026-10.md)). GitKraken Desktop (desde la 12.0, abr-2026) y GitLens 19 ya tienen vista de agentes con estado en vivo, así que "ver la flota" deja de ser diferencial. Se actualizan § 1, § 2 (P1 y P7), § 3, § 4, § 10 y los anexos. La propuesta de valor pone **"proteger y deshacer" por encima de "ver"** (hipótesis). No cambia ninguna prioridad de BR: los ajustes al backlog quedan como propuesta en el documento de research.
  - 0.8 (2026-10-06): D6 (decisión de Rene Bonilla): el autor de un commit es la persona y el agente va como trailer `Co-Authored-By`. Nueva **BR-26** (Should, Guardrails del MVP): política de autoría por repo (`agents-commit` por defecto, `human-author`, `flexible`), que separa quién ejecutó el commit de a nombre de quién entra. La pregunta abierta 5 anota que la auditoría local de autoría va en el núcleo y su exportación sigue a BR-24.
---

# GitRaptor — Documento de Negocio

> **El copiloto de Git para equipos que programan con agentes de IA.**
> GitRaptor hace recuperable lo que hacen tus agentes en el repo, también con Git crudo y el trabajo sin commitear, y frena las operaciones de Git peligrosas antes de que salgan de tu máquina. Además te avisa antes de que dos agentes choquen. Funciona en local y sin cuenta, desde la terminal, el editor o el propio agente vía MCP. En el MVP detecta Claude Code; Codex, Cursor y Copilot vienen después (D2).

Este documento es la entrada para la fase de análisis (requerimientos detallados, historias de usuario) y la de diseño e implementación.

---

## 1. Resumen ejecutivo

En 2026 programar dejó de ser "un dev, una rama, un editor". Hoy es común tener **3 a 10 agentes trabajando en paralelo**, cada uno en su worktree o rama, generando commits más rápido de lo que un humano puede revisar. Eso trae tres problemas. Desde 2026 los clientes Git ya muestran a los agentes (GitKraken Desktop desde la 12.0 de abril de 2026 y GitLens 19), así que la visibilidad empieza a estar cubierta; la seguridad y el control, no:

1. **Visibilidad:** no sé qué agente está tocando qué, ni si dos agentes van a chocar.
2. **Seguridad:** miedo a que un agente haga `reset --hard`, un force-push o borre trabajo, y a no poder volver atrás.
3. **Control:** revisar, integrar o descartar el trabajo de muchos agentes es lento y manual.

**GitRaptor** es un **motor local de Git consciente de los agentes** con varias superficies encima:

| Superficie | Para quién | Fase |
|---|---|---|
| **CLI + TUI** (`raptor`) | El dev en la terminal, al lado de Claude Code o Codex | MVP |
| **Servidor MCP** | Los agentes mismos (Cursor, Claude Code, Copilot, Codex) | MVP |
| **Vista visual** (extensión VS Code/Cursor y/o app de escritorio) | Quien prefiere UI gráfica, demos y tech leads | Fase 3 |

Tiene tres pilares de producto:

- 🛰️ **Cockpit:** panel en vivo de agentes, worktrees y ramas, con **predicción de conflictos entre agentes**.
- ⏪ **Time Machine:** snapshots automáticos antes de cada acción y **undo de cualquier cosa**, incluido "deshaz lo que hizo el agente X en los últimos 20 min".
- 🛡️ **Guardrails:** políticas por repo que los agentes no pueden saltarse (no push a main, no force-push, convenciones de commit), aplicadas en el MCP y en los hooks de Git.

**Diferenciación (v0.7):** el espacio ya tiene jugadores (sección 3), y ver la flota de agentes ya lo hacen varios, GitKraken incluido. Ninguno junta las cuatro cosas siguientes:

1. **Recupera lo que hagan los agentes**, también con **Git crudo** (hasta el último estado capturado) y el **trabajo sin commitear**, no solo la última acción hecha en una app.
2. **Guardrails con semántica de Git**, en los hooks de Git (frenan a cualquier herramienta que los ejecute; si alguien los desactiva, te enteras) y en un servidor MCP con políticas.
3. **Local, sin cuenta y gratis** para uso interno, con **cualquier agente** y **Git nativo** (sin modelo propio de ramas ni hosting propio).
4. **Predicción de conflictos local** entre worktrees y con la base, incluido el solape de lo sin commitear.

Windows y Azure DevOps se mantienen como requisitos (BR-03, BR-19), pero ya no diferencian por sí solos: GitKraken y otros los tienen.

---

## 2. Problema y contexto

| # | Problema | Quién lo sufre | Evidencia / contexto 2026 |
|---|----------|----------------|---------------------------|
| P1 | No hay una vista unificada de qué hace cada agente: rama, worktree, archivos tocados y si sigue corriendo o ya terminó. | Devs y tech leads que usan agentes en paralelo | Auge de orquestadores locales (Conductor, Claude Squad, Superset). Addy Osmani: "3-10 agentes" como caso típico. **v0.7:** este problema ya lo atacan GitKraken Desktop (vista Agents), GitLens 19, Kepler y las vistas nativas de Claude Code, Cursor, Codex y Copilot. |
| P2 | Los conflictos entre agentes se descubren tarde, al hacer merge. | Equipos con agentes en paralelo | Claude Code Agent Teams añadió *file locking* experimental, lo que confirma el dolor. |
| P3 | Miedo a acciones destructivas de agentes (reset, force-push, borrado de ramas) y a la *prompt injection*. | Todos los usuarios de agentes | Vulnerabilidades de `mcp-server-git` (ene-2026). Aparecen proxies de políticas (Intercept) y gates (Reflex). |
| P4 | Deshacer el trabajo de un agente es manual: reflog, cherry-pick, adivinar. | Devs junior y semi-senior, y también seniors | Entire Checkpoints y GitButler apuestan por los snapshots y el rewind. |
| P5 | Los agentes generan diffs gigantes, difíciles de revisar e integrar. | Revisores y tech leads | Tendencia hacia PRs apilados y commits pequeños. |
| P6 | En empresa no hay gobierno ni auditoría de qué cambió un agente y bajo qué reglas. | CTO, seguridad, compliance | Entire levantó USD 60M para "agent provenance". Futurum: la procedencia es el activo estratégico. |
| P7 | Las herramientas actuales son **solo Mac** (Conductor, Superset), **sin Windows** (Claude Squad usa tmux), **atadas a un agente**, **atadas a GitHub/Linear** o **exigen cuenta y plan de pago** para usar agentes en repos privados (GitKraken). | Equipos corporativos en Windows o con Azure DevOps | Ver sección 3.2. GitKraken sí tiene Windows y Azure DevOps (plan Pro). |

---

## 3. Research de mercado

> **Actualizado en v0.7 (2026-10-05)** con la investigación [RES-GRP-COMP-2026-10](research/competitive-2026-10.md), que tiene la matriz de funciones completa y las fuentes con fecha. Los datos de esta sección se resumen de ahí.

### 3.1 Mapa del mercado

El eje que más importa en 2026 ya no es "clásico frente a agent-aware": casi todos se volvieron agent-aware. Lo que separa a GitRaptor es **ver** frente a **proteger y deshacer**.

```
                       Proteger y deshacer
                       (Git crudo, sin commitear, políticas)
                               ▲
                               │        ◎ GitRaptor
             jj (op log) ●     │
     GitButler (oplog) ●       │
                               │
 Terminal / headless ──────────┼──────────────────► Visual / GUI
                               │
      Claude Squad ●           │     ● Conductor (checkpoints por turno)
       Clash (conflictos) ●    │     ● GitKraken Desktop / GitLens / Kepler
 Claude Code, Codex (nativo) ● │     ● Nimbalyst / Superset
                               │     ● Cursor / Copilot (nativo)
                               ▼
                             Ver la flota

  ◎ GitRaptor: terminal + MCP primero y visual después; local, sin cuenta,
    agnóstico del agente y Git nativo; undo de cualquier cosa y guardrails.
```

### 3.2 Competidores directos (agentes + Git)

| Herramienta | Qué hace | Modelo | Fortalezas | Debilidades / hueco para GitRaptor |
|---|---|---|---|---|
| **GitKraken Desktop + GitLens + Kepler** (GitKraken) | Desde Desktop 12.0 (abr-2026): vista **Agents** con una tarjeta por worktree, estado en vivo de Claude Code, Codex, Copilot CLI y OpenCode, botón para lanzar el agente, PRs y, desde la 12.4, aprobación de permisos del agente. GitLens 19 trae lo mismo a VS Code. Kepler es su entorno de agentes (preview). Undo de la última acción hecha en la app. Conflict Prevention sobre lo commiteado. CLI `gk` con servidor MCP de 22 herramientas | Free solo con repos locales y públicos. Pro (~USD 10, no verificado), Advanced USD 14, Business ~USD 18 por usuario y mes. Requiere cuenta | Cliente Git maduro, multiplataforma, Azure DevOps (Pro), base instalada enorme, ritmo de lanzamiento mensual | Undo solo de **la última acción hecha en su app** (no Git crudo ni lo que hace un agente por terminal, no verificado). MCP **sin políticas documentadas** (expone `git_push`). Conflictos solo de lo commiteado, de pago y con nube entre compañeros. Exige cuenta y plan Pro para agentes en repos privados. **Es el líder en "ver"; la convivencia es posible: GitKraken para ver, GitRaptor para proteger y deshacer.** |
| **Conductor** (Melty Labs) | App que corre Claude Code, Codex, Cursor y OpenCode en paralelo, un workspace por tarea, con diff, PR y **checkpoint antes de cada turno** | Free, Pro USD 50/mes, Teams USD 60/usuario/mes. Closed source | UX pulida, checkpoints por turno con lo sin commitear | **Solo macOS**. Sin políticas (solo aprobación por herramienta). Cobertura del Git crudo no verificada. |
| **Claude Squad** | TUI en Go: tmux + un worktree por agente | Open source, AGPL-3.0 | Terminal-first, multi-agente | **Sin Windows nativo**. Sin predicción de conflictos, políticas ni undo. AGPL complica el uso empresarial. |
| **GitButler** | Cliente Git con *virtual branches*, pestaña de agentes, CLI `but`, skill y MCP, oplog con snapshots | FSL-1.1-MIT. Serie A de USD 17M (abr-2026) | Varios agentes en **un solo directorio**, commit automático por prompt, oplog que incluye lo sin commitear, Windows | Te obliga a **adoptar su modelo** (virtual branches). Sin Azure DevOps. MCP sin políticas (no verificado). |
| **Entire** (Thomas Dohmke, ex-CEO de GitHub) | Checkpoints: guarda la sesión del agente en cada commit (rama oculta), shadow branches para rewind, Entire Blame y Review, red Git distribuida | Open source en el CLI. Plataforma y hosting de pago (precios sin anunciar). | Muy bien financiado (USD 60M), visión de procedencia | Va hacia **hosting y plataforma propia**. No es un cockpit local ni tiene guardrails. **Es un posible aliado:** se pueden leer sus checkpoints. |
| **Nimbalyst** (sucesor de Crystal) | App para sesiones paralelas, kanban, historial de archivos con snapshot antes y después de cada edición de la IA | MIT. Gratis individual. Teams USD 20/usuario/mes. | Kanban, gestión de tareas, Windows | Crystal quedó deprecado (feb-2026). Enfoque en tareas, no en Git. Sin políticas. |
| **Vibe Kanban** | Kanban en el que cada tarjeta lanza un agente en su rama | Apache-2.0, comunitario | Planificación visual | La empresa cerró (abr-2026). Mantenimiento incierto. |
| **Superset, Emdash, Sculptor, Orca, Warp, Zed, Amp, ccmanager, Container Use, mux y otros** | Orquestadores multi-agente con worktrees | Varios | Multi-agente | Mercado fragmentado y poco diferenciado. Ninguno documenta políticas Git ni undo de Git crudo. Terragon cerró (feb-2026). |
| **Clash** | CLI que **predice conflictos entre worktrees** con un merge simulado | MIT | Competidor directo del pilar de predicción | Solo predicción: sin undo, sin políticas, sin vista de agentes. |
| **Funciones nativas: Claude Code, Cursor, Codex, Copilot** | Worktrees y paralelo (`claude -w`, Agents Window, app de Codex, Copilot app), vistas de flota (`claude agents`, mission control), hooks y managed settings | Incluidas en el agente | Sin fricción, cada vez más completas | **Cada una atada a su agente.** Su undo **no cubre Git crudo**: `/rewind` de Claude Code excluye Bash, Cursor y Copilot solo restauran archivos y Codex retiró `/undo`. Sin predicción de conflictos. |

### 3.3 Competidores adyacentes

| Categoría | Ejemplos | Relación con GitRaptor |
|---|---|---|
| Servidores MCP de Git | `mcp-server-git` (Anthropic), git-mcp (self.agency), GitHub MCP Server, **MCP de GitKraken (`gk mcp`)** | Hacen falta, pero son "Git crudo" con guardrails mínimos. `mcp-server-git` tuvo CVEs en ene-2026. El MCP de GitKraken expone `git_push` y `git_checkout` sin políticas documentadas y exige cuenta. **GitRaptor MCP compite aquí con herramientas de alto nivel y políticas, y sus hooks de Git frenan también a los agentes que usan otro MCP** (si ese MCP ejecuta el Git del sistema). |
| Capas de política y seguridad | Intercept (proxy MCP con YAML), Reflex (gate de comandos), GitGuardian MCP | Genéricas, no entienden Git. **GitRaptor aplica políticas con semántica de Git** y puede convivir con ellas. |
| Clientes Git visuales | GitKraken, GitLens, Git Graph (abandonado), Git Graph Plus, GitBit, GitStudio | **v0.7:** GitKraken y GitLens **ya son agent-aware** y pasan a la tabla 3.2. El resto no lo es. Fuente de inspiración para la vista visual (Fase 3). Ver Anexo B. |
| Terminal | lazygit, tig, gitui | Referente de UX TUI. Ninguno sabe de agentes. |
| PRs apilados | Graphite (comprado por Cursor, dic-2025), PRs apilados de GitHub (preview, jul-2026), ghstack, git-branchless, Jujutsu (jj, con *operation log* y undo) | Inspiración para Time Machine (oplog de jj) y para Stacks (Fase 3). |

### 3.4 Conclusiones del research (v0.7)

1. **El dolor es real y validado por el mercado:** hay financiamiento (Entire, GitButler), lanzamientos continuos y funciones nativas en los agentes.
2. **"Ver la flota" ya es higiene.** El riesgo que la v0.6 marcaba como principal se materializó: GitKraken Desktop (12.0, abr-2026), GitLens 19 y las vistas nativas de Claude Code, Cursor, Codex y Copilot muestran los agentes y su estado. GitRaptor no compite en riqueza visual.
3. **El mercado sigue fragmentado e inestable** en los orquestadores (Crystal deprecado, Vibe Kanban y Terragon cerrados), pero **GitKraken consolida** la parte visual con un ritmo de lanzamiento mensual.
4. **Los huecos que siguen abiertos son:**
   - **Undo de cualquier cosa**, incluido el Git crudo de los agentes y el trabajo sin commitear. Nadie lo cubre: GitKraken deshace la última acción de su app y los undos nativos excluyen Git.
   - **Guardrails con semántica de Git** en hooks y en un MCP con políticas. El MCP de GitKraken no las documenta.
   - **Local, sin cuenta y gratis** con repos privados. GitKraken exige cuenta y plan Pro para agentes en repos privados.
   - **Predicción de conflictos local** entre worktrees, con el solape de lo sin commitear. GitKraken solo mira lo commiteado; Clash es un competidor directo en este pilar.
   - **Agnóstico del agente** en la protección (no en la detección: GitKraken ya detecta cuatro agentes).
5. **Windows y Azure DevOps ya no diferencian por sí solos** (GitKraken los tiene), aunque siguen siendo requisitos para los equipos corporativos.
6. **Convivir con GitKraken en lugar de competir de frente:** "GitKraken para ver, GitRaptor para proteger y deshacer". La captura continua observa también lo que se hace desde GitKraken; sus hooks de Git frenan un force-push desde su GUI (`pre-push`), pero no el borrado o el movimiento de ramas locales, porque GitKraken no ejecuta `reference-transaction` (se comprueba en el dogfooding).
7. **No competir con Entire en hosting ni en procedencia.** Mejor **integrarse** con sus checkpoints y estándares abiertos.

---

## 4. Propuesta de valor

> **"Pon a trabajar a 10 agentes en tu repo sin miedo. Lo que hagan es recuperable y las operaciones peligrosas de Git no pasan."**

**Hipótesis (v0.7, 2026-10-05):** el diferencial de GitRaptor es **proteger y deshacer**, por encima de **ver**. Ver la flota de agentes ya lo hacen GitKraken Desktop, GitLens y las vistas nativas de los agentes; el Cockpit se mantiene en versión mínima y se diferencia por la predicción de conflictos local. GitRaptor **convive** con esas herramientas: "GitKraken para ver, GitRaptor para proteger y deshacer". Decisión del orquestador (2026-10-05), validada por PO y Arquitecto; responde la pregunta abierta 1 (§ 12.2) y queda **pendiente de confirmar por Rene Bonilla**. Detalle y recomendaciones en [RES-GRP-COMP-2026-10](research/competitive-2026-10.md) § 5.

Mensaje: decir **"recuperable"**, no "todo reversible" (con Git crudo se recupera el último estado capturado); el diferencial es **local**, antes del push (el hosting ya protege sus ramas en el remoto).

| Diferenciador | GitKraken (Desktop, GitLens, `gk`) | Conductor | Claude Squad | GitButler | Entire | Funciones nativas del agente | **GitRaptor** |
|---|---|---|---|---|---|---|---|
| Recupera el Git crudo de un agente | ✗ (no verificado) | ? | ✗ | ? | ? | ✗ | **≈** (último estado capturado) |
| Recupera el trabajo sin commitear | ≈ | ✔ | ✗ | ✔ | ✔ | ≈ (solo archivos) | **✔** |
| Guardrails y políticas Git | ✗ | ✗ | ✗ | ✗ | ✗ | ≈ (hooks del agente) | **✔** (clientes que ejecutan hooks) |
| Servidor MCP con políticas | ✗ (sin políticas documentadas) | ✗ | ✗ | ✗ | — | — | **✔** |
| Predicción de conflictos local, con lo sin commitear | ≈ (solo commiteado, de pago) | ? | ✗ | ≈ | ✗ | ✗ | **✔** |
| Local, sin cuenta y gratis con repos privados | ✗ (cuenta y Pro) | ? | ✔ | ✔ | ≈ | ✗ | **✔** |
| Git nativo (sin modelo propio) | ✔ | ✔ | ✔ | ✗ | ≈ | ✔ | **✔** |
| Cockpit en vivo de agentes | ✔ (4 agentes) | ✔ | ✔ | ✔ | ✗ | ✔ | **✔** (mínimo; Claude Code en el MVP) |
| Multiplataforma (Win/Mac/Linux) | ✔ | ✗ | ✗ | ✔ | ✔ | ✔ | **✔** (objetivo; hoy solo macOS verificado) |
| Azure DevOps | ✔ (Pro) | ? | ? | ✗ | ? | ≈ | **Fase 2** (BR-19) |

✔ sí · ≈ parcial · ✗ no · ? no verificado · — no aplica. Las celdas de GitRaptor son compromisos del backlog (pre-MVP).

---

## 5. Usuarios objetivo

| Persona | Descripción | Necesidad principal |
|---|---|---|
| **"Agent wrangler"** | Dev senior que lanza 3-10 agentes en paralelo desde la terminal, a menudo con un cliente Git que ya le muestra los agentes (GitKraken, GitLens) | Recuperar lo que un agente rompa, saber quién choca con quién, integrar rápido |
| **Dev junior o semi-senior con agentes** | Usa Cursor o Claude Code pero le da miedo que el agente rompa algo | Red de seguridad: undo de un comando |
| **Tech lead / revisor** | Revisa y aprueba el trabajo de humanos y agentes | Diffs pequeños, contexto de qué hizo el agente, aprobar o descartar |
| **Platform / DevEx engineer** | Define cómo trabaja la organización con agentes | Políticas centralizadas, auditoría, integración con Azure DevOps |
| **El agente de IA** (usuario no humano) | Claude Code, Cursor, Codex, Copilot vía MCP | Herramientas Git seguras y de alto nivel, contexto del repo y límites claros |

---

## 6. Alcance funcional (alto nivel)

Prioridad MoSCoW. Los IDs (`BR-xx`) se descomponen en historias de usuario en la fase de análisis.

### 6.1 MVP — Fase 1: "Cockpit + Time Machine + Guardrails" (CLI/TUI + MCP)

**Núcleo / motor**

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-01 | **Motor local** (`raptord` o una librería) que observa uno o varios repos: worktrees, ramas, estado del working tree, procesos de agentes y eventos de Git. | Must |
| BR-02 | **Detección de agentes:** identificar sesiones de Claude Code, Cursor, Codex y Copilot por worktree o rama (procesos, hooks, convenciones de nombres) y permitir el registro explícito vía CLI o MCP. | Must |
| BR-03 | **Multiplataforma** con un binario único para Windows, macOS y Linux. Sin dependencia de tmux. | Must |

**🛰️ Cockpit**

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-04 | **TUI en vivo** (`raptor`): lista de agentes y worktrees con rama, estado (activo/inactivo/terminado), archivos modificados, commits ahead/behind y última actividad. | Must |
| BR-05 | **Grafo en vivo** en la TUI: ramas de agentes creciendo en tiempo real sobre la rama base. | Must |
| BR-06 | **Predicción de conflictos:** detectar solapamiento de archivos y hunks entre agentes, y con la rama base, antes del merge (merge-tree en seco) y alertar. | Must |
| BR-07 | **Acciones por agente:** ver el diff, abrirlo en el editor, aprobar y hacer merge o rebase a la base, descartar (borrar worktree y rama), crear worktree para un nuevo agente. | Must |

**⏪ Time Machine**

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-08 | **Snapshots automáticos** (refs ocultas y oplog propio) antes de cada operación que modifica estado, la haga un humano o un agente, incluido el working tree sin commitear. | Must |
| BR-09 | **Undo / Redo universal:** `raptor undo`, `raptor undo --agent <id> --since 20m`, y restaurar a cualquier punto del timeline. | Must |
| BR-10 | **Timeline**: qué cambió, cuándo y quién (humano o agente X), con una vista navegable en la TUI. | Must |

**🛡️ Guardrails**

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-11 | **Políticas por repo** en la configuración versionable del repo (carpeta `.gitraptor/`; formato y estructura en un ADR de arquitectura): ramas protegidas, prohibir force-push o `reset --hard`, límite de tamaño de diff, formato de commit, rutas prohibidas. | Must |
| BR-12 | **Aplicación de políticas** en dos capas: (a) en las herramientas MCP, que rechazan antes de ejecutar; (b) en los hooks de Git (pre-commit, pre-push, reference-transaction), para cubrir a los agentes que usan Git crudo. | Must |
| BR-13 | **Modo "pedir confirmación":** las acciones de riesgo de un agente quedan en cola para que un humano las apruebe en la TUI. | Should |
| BR-26 | **Política de autoría de los commits** (D6): GitRaptor acepta commits de personas y de agentes. Por defecto el autor es la persona y el agente va como trailer `Co-Authored-By`. Variantes por repo: `agents-commit` (el agente hace commits con el trailer exigido; valor por defecto), `human-author` (el agente no hace commits: bloquear o avisar) y `flexible` (solo registrar). Distingue **quién ejecutó** el commit (proceso, sesión, worktree) de **a nombre de quién entra** (autor, committer, trailer), y muestra las dos cosas cuando difieren. | Should |

**🤖 Servidor MCP**

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-14 | **GitRaptor MCP** con herramientas de alto nivel y seguras: `status`, `explain_history`, `safe_commit`, `safe_rebase`, `create_worktree`, `check_conflicts`, `snapshot` y `undo`, todas sujetas a las políticas de BR-11. | Must |
| BR-15 | **Instalación en un paso** para Claude Code, Cursor, VS Code/Copilot y Codex (`raptor mcp install`). | Must |
| BR-16 | **Endurecimiento de seguridad:** allowlist de repos, protección contra path traversal, validación de refs, argv fijo (sin shell) y límites de salida, aprendiendo de los CVEs de `mcp-server-git`. | Must |

### 6.2 Fase 2: "Revisión e integración"

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-17 | **Review queue:** cola de ramas de agentes listas para revisar, con un resumen (AI opcional) de qué hizo cada una y por qué. | Should |
| BR-18 | **Split / Stacks:** dividir un diff grande de un agente en commits o PRs apilados y hacer restack automático cuando cambia la base. | Should |
| BR-19 | **Creación de PRs** en **Azure DevOps** y GitHub desde el cockpit, enlazando work items. | Should |
| BR-20 | **Integración con Entire Checkpoints** y metadatos de sesión: enlazar commit, sesión del agente y prompt (blame con contexto). | Could |
| BR-21 | **Gestión de recursos por worktree:** puertos y variables de entorno por agente, para que no choquen dev servers ni bases de datos. | Could |

### 6.3 Fase 3: "Visual + empresa"

| ID | Capacidad | Prioridad |
|---|---|---|
| BR-22 | **Vista visual:** extensión de VS Code/Cursor (Marketplace + Open VSX) y/o app de escritorio (Tauri) con un grafo animado en vivo de agentes. Consume el mismo motor. | Should |
| BR-23 | **Políticas centralizadas** para la organización: herencia de políticas y distribución desde un repo central. **Edición de equipo** (D4). | Should |
| BR-24 | **Auditoría:** exportar el log de acciones de agentes y de violaciones de política (JSON y SIEM). | Should |
| BR-25 | **Dashboard de equipo** (opcional, self-hosted): actividad de agentes por repo y métricas. **Edición de equipo** (D4). | Could |

> **Edición de equipo (D4):** BR-23 y BR-25 se entregarán con licencia comercial. El núcleo gratuito (FSL-1.1-ALv2) cubre lo que D4 enumera: motor, CLI/TUI, MCP, Time Machine y Guardrails individuales. BR-22, BR-24 y la parte avanzada de Azure DevOps (BR-19) no tienen edición asignada todavía (pregunta abierta 5).

### 6.4 Fuera de alcance (por ahora)

- Ejecutar o orquestar los agentes (lanzar prompts, gestionar modelos). GitRaptor **observa y protege**; no reemplaza a Claude Code, Cursor ni Codex.
- Hosting de repositorios o una red Git propia (el terreno de Entire y GitHub).
- Un modelo de ramas propio (virtual branches estilo GitButler).
- VCS distintos de Git.

---

## 7. Requerimientos no funcionales (de negocio)

| ID | Categoría | Requerimiento |
|---|---|---|
| NFR-01 | Seguridad | **Cero pérdida de datos:** toda operación destructiva genera antes un snapshot recuperable. |
| NFR-02 | Seguridad | El servidor MCP no ejecuta comandos vía shell, valida todas las entradas y opera solo sobre repos de la allowlist. Pasa una revisión de seguridad (OWASP / MCP Top 10) antes de cada release. |
| NFR-03 | Seguridad | 100% local. No envía código ni metadatos fuera de la máquina. La telemetría es opt-in. |
| NFR-04 | Rendimiento | La TUI refleja cambios en **< 500 ms**. Overhead del snapshot **< 200 ms** por operación en repos medianos. |
| NFR-05 | Rendimiento | Soporta **10 o más worktrees activos** y repos de más de 100K commits sin degradarse. |
| NFR-06 | Portabilidad | Binario único para Windows, macOS y Linux (x64 y arm64). Instalación por `winget`, `brew`, `npm`/`npx` y script. |
| NFR-07 | Compatibilidad | Usa el Git del sistema (≥ 2.38 por `merge-tree --write-tree`) y respeta la config, los hooks y las credenciales del usuario. |
| NFR-08 | Interoperabilidad | Compatible con MCP estándar. Agnóstico del agente. Sin APIs privadas de ningún IDE. |
| NFR-09 | UX | TUI navegable por teclado, con temas y accesible (sin depender solo del color). |
| NFR-10 | i18n | CLI y TUI en inglés y español. |
| NFR-11 | Legal | Licencia permisiva o open-core apta para empresa (no AGPL en el núcleo). Clean-room respecto a los competidores. **Resuelto por D4:** núcleo bajo FSL-1.1-ALv2. |
| NFR-12 | Calidad | Suite de integración contra repos reales con escenarios multi-agente simulados y pruebas de caos (kill del proceso a mitad de una operación). |

---

## 8. Modelo de negocio

| Opción | Descripción | Pros | Contras |
|---|---|---|---|
| **A. Open source total** | Todo MIT o Apache-2.0 y sponsors | Adopción y confianza máximas | Sin ingresos |
| **B. Open-core** (recomendada) | **Gratis:** CLI/TUI, MCP, Time Machine y políticas locales. **De pago (Teams/Enterprise):** políticas centralizadas, auditoría, dashboard, Azure DevOps avanzado y soporte. | La adopción de los devs alimenta la venta a empresa. El comprador (CTO o seguridad) tiene un problema de gobierno real. | Hay que mantener la frontera entre los niveles |
| **C. Herramienta interna** | Uso corporativo (ASSA) | Control total, ajuste a nuestro stack | Sin tracción externa |

**Decisión (v0.3):** opción **C, herramienta interna al inicio**. La arquitectura y la licencia se mantienen compatibles con una futura opción **B (open-core)**, que se reevalúa cuando el MVP esté estable y probado internamente.

**Decisión (v0.6, 2026-10-05, Rene Bonilla):** el modelo de licencia y monetización es la opción **B, open core** (D4):

| Edición | Qué incluye | Licencia | Precio |
|---|---|---|---|
| **Núcleo** | Motor, CLI/TUI (`raptor`), servidor MCP, Time Machine y Guardrails individuales (políticas por repo y locales) | [FSL-1.1-ALv2](../../LICENSE) (Functional Source License 1.1; cada versión pasa a Apache-2.0 a los dos años) | Gratis para cualquier usuario |
| **Equipo** (más adelante) | Administración de grupos de usuarios, políticas centralizadas (BR-23) y dashboard de equipo (BR-25) | Comercial | De pago |

- La FSL permite cualquier uso salvo el "uso competidor": ofrecer el software a terceros en un producto o servicio comercial que lo sustituya o que dé la misma funcionalidad o una sustancialmente similar. El uso interno en una empresa está permitido, así que se cumple NFR-11.
- El núcleo es *source-available* (Fair Source), no open source aprobado por la OSI, hasta que cada versión pasa a Apache-2.0.
- Decisión del orquestador (2026-10-05), validada por el PO: solo BR-23 y BR-25 se marcan como edición de equipo; BR-19, BR-22 y BR-24 pasan a la pregunta abierta 5, y la administración de grupos de usuarios no recibe BR hasta especificar la Fase 3.
- La edición de equipo es Fase 3 y **no se construye todavía**. La administración de grupos de usuarios no tiene BR propio: se escribe cuando se especifique la Fase 3.
- La publicación externa ocurre cuando el MVP esté estable internamente (D1); D4 define la licencia con la que se publicará. Publicar en los canales sigue siendo un paso humano (ADR-GRP-014).
- Antes del lanzamiento comercial se recomienda una revisión legal: la licencia, la frontera entre ediciones y quién es el titular de la propiedad intelectual (el `LICENSE` nombra como licenciante a Rene Bonilla; la opción C hablaba de uso corporativo en ASSA).

---

## 9. KPIs y criterios de éxito

Como el producto arranca como **herramienta interna**, los KPIs miden uso y valor real, no adopción pública:

| KPI | Meta a 6 meses desde el MVP |
|---|---|
| Dogfooding: GitRaptor se usa para construir GitRaptor | 100% de las sesiones de desarrollo con agentes |
| Desarrolladores internos usándolo cada semana | ≥ 5 |
| Repos internos con GitRaptor y políticas configuradas | ≥ 5 |
| Undos ejecutados por usuario activo y mes | ≥ 3 (señal de que la red de seguridad se usa) |
| Acciones peligrosas bloqueadas por guardrails | Se mide; es el argumento de venta |
| Conflictos entre agentes detectados antes del merge | ≥ 70% de los que luego ocurren |
| Incidentes de pérdida de datos causados por GitRaptor | **0** |
| Equipos internos piloto | 2 |
| Tiempo para integrar el trabajo de un agente (revisar → merge) | −30% frente a la línea base sin GitRaptor |

---

## 10. Riesgos

| Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|
| Los agentes (Cursor, Claude Code) o GitLens 19 incorporan funciones equivalentes de forma nativa | **Materializado en visibilidad** (v0.7) · Alta en el resto | Alto | **Materializado en "ver la flota":** GitKraken Desktop 12, GitLens 19 y las vistas nativas ya lo hacen. Mitigación: diferenciarse en **proteger y deshacer** (undo de Git crudo, guardrails, MCP con políticas), mantener el Cockpit mínimo y **convivir** con GitKraken. Los guardrails y la auditoría empresarial difícilmente vienen de un agente en particular. Repetir la investigación competitiva cada trimestre ([RES-GRP-COMP-2026-10](research/competitive-2026-10.md)). |
| GitKraken añade undo de Git crudo o políticas en su MCP | Media | Alto | Llegar primero con Time Machine y Guardrails, y ser local, sin cuenta y gratis. Vigilar sus notas de versión (lanzamiento mensual). |
| Entire u otro jugador financiado ocupa la categoría | Media | Alto | No competir en hosting ni en procedencia: integrarse. Enfocarse en local, guardrails y Azure DevOps. |
| Un bug de GitRaptor causa pérdida de datos | Media | Crítico | NFR-01, pruebas de caos, snapshots antes de cada operación y releases graduales. |
| El servidor MCP se vuelve un vector de ataque (prompt injection, RCE) | Media | Crítico | NFR-02, revisión de seguridad por release, argv fijo y allowlists. |
| Los agentes ignoran el MCP y usan Git crudo | Alta | Medio | Doble capa: hooks de Git (BR-12) más MCP. |
| La detección de agentes es frágil (cambian procesos y convenciones) | Media | Medio | Registro explícito (BR-02) y adaptadores por agente fáciles de actualizar. |
| Mercado inestable (cierres de Vibe Kanban y Crystal) | Media | Medio | Un MVP enfocado en el dolor más claro (undo + guardrails) y no en orquestación. |
| Overhead de rendimiento por los snapshots | Media | Medio | Snapshots incrementales basados en objetos de Git y un spike temprano. |
| **Una sola persona revisa todo lo que producen varios agentes** (cuello de botella y riesgo de calidad) | Alta | Alto | Historias pequeñas con criterios de aceptación verificables, tests obligatorios, code review automatizado por agente, CI como gate y el propio GitRaptor (guardrails + undo) protegiendo el repo. |
| Pérdida de contexto entre sesiones de agentes | Media | Medio | Documentación como fuente de verdad (BRD, ADRs, dev specs, historias) y un flujo AADD con artefactos en el repo. |

---

## 11. Supuestos y restricciones

- **Stack decidido en [ADR-GRP-001](../architecture/decisions/ADR-GRP-001-stack-tecnologico.md):** motor, CLI/TUI y MCP en **Rust** (gitoxide para leer y el Git CLI para escribir, ratatui, rmcp). App de escritorio en **Tauri + React**. Extensión en **TypeScript**. Monorepo **Nx package-based** ([ADR-GRP-002](../architecture/decisions/ADR-GRP-002-monorepo-nx.md)) con un **design system** propio ([ADR-GRP-003](../architecture/decisions/ADR-GRP-003-design-system.md), [docs/design-system](../design-system/README.md)) y la guía de estado del frontend y UX ([ADR-GRP-004](../architecture/decisions/ADR-GRP-004-estado-frontend-ux.md)). La decisión queda sujeta a validarse en el spike.
- Lo que falta decidir en ADRs de Arquitectura:
  - Diseño del oplog y los snapshots, inspirado en el *operation log* de Jujutsu y las shadow branches de Entire.
  - Mecanismo de detección de agentes.
  - Modelo de seguridad del MCP.
- El motor es la única fuente de verdad. La TUI, el MCP y la futura extensión o app son clientes del mismo motor (API local).
- **Equipo:** una persona (Rene Bonilla, producto + revisión + integración) orquestando **múltiples agentes de IA** (Claude Code y Cursor) que diseñan, implementan, prueban y revisan. No hay equipo humano adicional en el MVP.
- **Dogfooding desde el día uno:** GitRaptor se construye con agentes en paralelo, exactamente el caso de uso que resuelve. Cada fase se usa para desarrollar la siguiente.
- **Agentes soportados en el MVP:** **solo Claude Code**. Después se añaden uno por uno: primero Codex y luego Cursor (Copilot, más adelante). Mientras no tengan soporte completo, se aceptan como "otro agente" mediante registro explícito (aplica a BR-02 y BR-15).
- **Hosting:** el repositorio del producto vive en GitHub (`rbonillajr/gitRaptor`). La integración con Azure DevOps (BR-19) se mantiene para los repos internos donde se use la herramienta.

---

## 12. Decisiones y preguntas abiertas

### 12.1 Decisiones tomadas (v0.3; D4 y D5 en v0.6; D6 en v0.8)

| # | Pregunta | Decisión |
|---|---|---|
| D1 | ¿Producto público o herramienta interna? | **Herramienta interna al inicio** (opción C), sin cerrar la puerta a open-core (B). El modelo destino queda fijado en D4 (2026-10-05). |
| D2 | ¿Qué agentes son prioritarios en el MVP? | **Solo Claude Code** (revisada el 2026-10-03; antes: Claude Code y Cursor). Después, Codex y luego Cursor, uno por uno; Copilot más adelante. |
| D3 | ¿Capacidad del equipo? | **Sin equipo humano:** una persona orquestando múltiples agentes de IA. La planificación se hace por historias pequeñas y verificables, no por velocity de un equipo. |
| D4 | ¿Modelo de licencia y monetización? | **Open core** (decisión de Rene Bonilla, 2026-10-05). Núcleo gratuito para cualquier usuario bajo **FSL-1.1-ALv2**; más adelante, una **edición de equipo** de pago con licencia comercial (grupos de usuarios, BR-23 y BR-25). Ver sección 8. |
| D5 | ¿Nombre y comando? | **GitRaptor**, publicado como `gitraptor` en los canales (tap propio de Homebrew, winget y npm); el comando sigue siendo `raptor` (decisión de Rene Bonilla, 2026-10-05; ADR-GRP-014 § 6). |
| D6 | ¿A nombre de quién entra un commit que hace un agente? | **El autor es la persona y el agente va como trailer `Co-Authored-By`** (decisión de Rene Bonilla, 2026-10-06), porque los permisos y las credenciales de Git son del usuario y el agente actúa con ellos. Cada empresa puede ser más estricta o más flexible, porque su harness puede dejar o no que el agente haga commits: por eso la política es configurable por repo en Guardrails (BR-26: `agents-commit`, `human-author`, `flexible`). |

### 12.2 Preguntas abiertas

1. ¿Qué pesa más en el MVP: **Time Machine + Guardrails** (seguridad) o **Cockpit** (visibilidad)? Propuesta: los tres, en versión mínima. **Propuesta v0.7 (2026-10-05):** pesan más **Time Machine y Guardrails** ("proteger y deshacer"), con el Cockpit mínimo y la predicción de conflictos como su diferencial (§ 4 y [RES-GRP-COMP-2026-10](research/competitive-2026-10.md)). Pendiente de confirmar por Rene Bonilla.
2. ~~¿Nombre y licencia definitivos? ¿Se mantiene "GitRaptor" y el comando `raptor`?~~ Resuelta por D4 y D5 (2026-10-05).
3. ¿Hay una fecha objetivo para el MVP?
4. ¿Integramos con Entire Checkpoints desde temprano o esperamos a que el estándar madure?
5. ¿En qué edición van la vista visual (BR-22), la auditoría (BR-24) y la parte avanzada de Azure DevOps (BR-19)? La opción B original ponía la auditoría y Azure DevOps avanzado en el nivel de pago; D4 solo asigna BR-23 y BR-25 a la edición de equipo. **Nota v0.8 (2026-10-06)**: la auditoría de autoría de BR-26 (política, registro local y la presentación "commit de \<persona\> con \<agente\> · \<worktree\>") va en el núcleo gratuito como parte de Guardrails individuales (D4) y no depende de esta pregunta. Exportar esas entradas a JSON o SIEM sí es BR-24 y sigue la edición que se le asigne aquí (decisión del orquestador, 2026-10-06, validada por el PO).

---

## 13. Próximos pasos

1. ~~Validar este documento~~ y cerrar las decisiones principales (hecho en v0.3; quedan las preguntas de 12.2).
2. **Spikes técnicos (1-2 semanas):**
   - (a) Snapshot y undo del working tree con overhead menor a 200 ms.
   - (b) Predicción de conflictos entre N worktrees con `git merge-tree`.
   - (c) Detección de sesiones de Claude Code.
   - (d) Prototipo del MCP con una política que bloquee el force-push.
3. **Fase de análisis:** `/aadd-specify` con este BRD → `context.md` → historias de usuario por BR.
4. **Arquitectura:** `/aadd-architect` → overview, ADRs (lenguaje, Git CLI frente a librería, oplog, detección de agentes, modelo de seguridad del MCP) y NFRs técnicos.
5. **Demo "wow" para validar:** 4 agentes en paralelo, el cockpit mostrando un conflicto antes de que ocurra, un agente que intenta hacer force-push y queda bloqueado, y un `raptor undo --agent` que restaura todo.

---

## Anexo A — Fuentes del research (agentes + Git)

> **v0.7:** la investigación del 2026-10-05, con la matriz de funciones y más de 130 fuentes con fecha, está en [RES-GRP-COMP-2026-10](research/competitive-2026-10.md) § 7. Las fuentes de abajo son las de la v0.2–v0.6.

- [Addy Osmani — The Code Agent Orchestra](https://addyosmani.com/blog/code-agent-orchestra/) · [Top AI Coding Trends for 2026](https://beyond.addy.ie/2026-trends/)
- [DEV — Best Tools for Managing Parallel AI Coding Agents in 2026](https://dev.to/stravukarl/best-tools-for-managing-parallel-ai-coding-agents-in-2026-14l8) · [Nimbalyst — Agent management tools 2026](https://nimbalyst.com/blog/best-agent-management-tools-2026/) · [AgentsRoom — Multi-agent tools](https://agentsroom.dev/blog/best-multi-agent-coding-tools) · [Parallel Code — Multi-agent tools 2026](https://parallelcode.app/blog/multi-agent-coding-tools-2026/) · [Abralo — alternatives](https://abralo.com/alternatives)
- [Conductor.build intro](https://codepick.dev/en/guides/conductor-build-intro/) · [Superset](https://superset.sh/)
- [GitButler — Agents tab](https://blog.gitbutler.com/agents-tab) · [GitButler 0.16](https://blog.gitbutler.com/gitbutler-0-16) · [GitButler Agent Assist](https://blog.gitbutler.com/gitbutler-agent-assist) · [Parallel Claude Code sin worktrees](https://blog.gitbutler.com/parallel-claude-code) · [gitbutlerapp/claude](https://github.com/gitbutlerapp/claude) · [Trigger.dev — ditched worktrees](https://trigger.dev/blog/parallel-agents-gitbutler)
- [Entire — seed round](https://entire.io/news/former-github-ceo-thomas-dohmke-raises-60-million-seed-round) · [RCP Mag — Entire](https://rcpmag.com/articles/2026/02/12/ex-github-ceo-thomas-dohmke-unveils-entire.aspx) · [OSTechNix — Entire CLI](https://ostechnix.com/entire-cli-git-observability-ai-agents/) · [Futurum — Agent provenance](https://futurumgroup.com/insights/selling-agent-provenance-to-the-cio-entire-changes-who-signs/) · [Entire distributed Git network waitlist](https://windowsforum.com/threads/entire-opens-waitlist-for-distributed-git-network-on-july-8.435946/) · [jd:/dev — Agent-written code needs more than Git](https://julien.danjou.info/blog/github-wont-work-for-ai-agents/) · [Crítica: "nobody asked for"](https://chyshkala.com/blog/github-s-ex-ceo-raises-60m-for-ai-agent-version-control-that-nobody-asked-for)
- [PointGuard AI — mcp-server-git vulnerabilities](https://www.pointguardai.com/ai-security-incidents/git-happens-mcp-flaws-open-door-to-code-execution) · [AgentSeal — mcp-server-git score](https://agentseal.org/mcp/mcp-server-git) · [git-mcp (self.agency)](https://git-mcp.self.agency/) · [github-mcp-server security policy #2136](https://github.com/github/github-mcp-server/issues/2136) · [Reflex MCP guardrails PR](https://github.com/ursuciprian/reflex/pull/73) · [GitGuardian MCP](https://blog.gitguardian.com/shifting-security-left-for-ai-agents-enforcing-ai-generated-code-security-with-gitguardian-mcp/) · [MCP ecosystem 2026](https://codeongrass.com/blog/mcp-server-ecosystem-integration-layer-ai-agents-2026/)

> Nota: varios rankings de herramientas multi-agente los publican competidores (Nimbalyst, AgentsRoom, Abralo, Parallel Code, Superset), así que pueden estar sesgados. Los precios de terceros no están verificados.

## Anexo B — Research previo: extensiones Git para VS Code / Cursor (v0.1, actualizado en v0.7)

Se mantiene como insumo para la vista visual de la Fase 3 (BR-22). **Actualización v0.7 (2026-10-05):** GitKraken y GitLens ya no son solo referencia visual; son competidores directos en la parte de "ver" (ver § 3.2 y [RES-GRP-COMP-2026-10](research/competitive-2026-10.md) § 2).

- **GitLens** (GitKraken): suite muy completa. Commit Graph, Visual History, Worktrees y AI son **Pro en repos privados**. v17.12 (abr-2026) añadió un sidebar al grafo. **v0.7:** GitLens 18 muestra el estado de Claude Code con hooks propios; la 19.0 (12-ago-2026) hace del Commit Graph la vista principal, con undo en un clic de sus operaciones; la 19.1 (1-sep-2026) suma Codex, Copilot CLI y OpenCode y "Start Agent Session" en worktrees.
- **Git Graph (mhutchie)**: popular pero **abandonado** desde ~2021, con una licencia que restringe derivados → clean-room obligatorio.
- **Git Graph Plus, GitBit, GitLG, GitStudio**: alternativas activas con pocos usuarios.
- **VS Code nativo**: Source Control Graph (desde v1.93) y worktrees (desde jul-2025), lo que sube el mínimo que hay que ofrecer.
- **Cursor usa Open VSX**: hay que publicar en ambos registros, con el namespace verificado y la verificación de publisher de Cursor.
- **GitKraken Desktop**: referencia de UX (drag & drop, rebase interactivo, undo, workspaces, Launchpad). **v0.7:** desde la 12.0 (14-abr-2026) tiene la vista **Agents** (tarjeta por worktree, estado en vivo de la sesión, lanzar el agente, PRs) y desde la 12.4 (5-ago-2026) aprueba o deniega permisos del agente desde la tarjeta. Su undo cubre solo la última acción hecha en la app. Las funciones de agentes en repos privados requieren plan Pro y cuenta.

Fuentes: [GitLens v17](https://help.gitkraken.com/gitlens/gl-release-v17-x/) · [GitLens Pro](https://gitkraken.com/gitlens/pro-features) · [GitKraken Desktop](https://gitkraken.com/git-client) · [Git Graph #927](https://github.com/mhutchie/vscode-git-graph/issues/927) · [Git Graph Plus](https://open-vsx.org/extension/the0807/git-graph-plus/changes) · [GitBit](https://open-vsx.org/extension/filipstrand/gitbit) · [GitStudio](https://gitstudio.dev/extensions) · [VS Code SCM history](https://code.visualstudio.com/docs/sourcecontrol/history) · [Cursor Extensions](https://cursor.com/help/customization/extensions) · [Open VSX Publishing](https://github.com/eclipse-openvsx/openvsx/wiki/Publishing-Extensions)
