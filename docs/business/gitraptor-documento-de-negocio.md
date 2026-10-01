---
id: BRD-GRP-001
title: GitRaptor — Documento de Negocio
type: business-requirements
status: draft
version: 0.3
date: 2026-10-01
author: Rene Bonilla
tags: [git, ai-agents, worktrees, mcp, tui, cli, agent-cockpit, safety-net, guardrails, azure-devops, brd]
changelog:
  - 0.1 (2026-10-01): Extensión VS Code/Cursor estilo GitKraken.
  - 0.2 (2026-10-01): Pivote a "Git para la era de los agentes de IA": motor local con CLI/TUI + servidor MCP; la extensión y la app de escritorio pasan a ser capas visuales posteriores.
  - 0.3 (2026-10-01): Decisiones cerradas: herramienta interna al inicio, Claude Code + Cursor como agentes del MVP, desarrollo por una persona orquestando múltiples agentes de IA (dogfooding desde el día uno).
---

# GitRaptor — Documento de Negocio

> **El copiloto de Git para equipos que programan con agentes de IA.**
> GitRaptor muestra en vivo qué están haciendo tus agentes en el repo, impide que rompan algo y te deja deshacer cualquier cosa. Funciona con cualquier agente (Claude Code, Cursor, Codex, Copilot), en cualquier sistema operativo y desde la terminal, el editor o el propio agente vía MCP.

Este documento es la entrada para la fase de análisis (requerimientos detallados, historias de usuario) y la de diseño e implementación.

---

## 1. Resumen ejecutivo

En 2026 programar dejó de ser "un dev, una rama, un editor". Hoy es común tener **3 a 10 agentes trabajando en paralelo**, cada uno en su worktree o rama, generando commits más rápido de lo que un humano puede revisar. Eso trae tres problemas que las herramientas Git clásicas (GitKraken, GitLens, Git Graph) no resuelven:

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

**Diferenciación:** el espacio ya tiene jugadores (sección 3), pero ninguno junta las cuatro cosas siguientes:

1. Funciona con **cualquier agente** y **en cualquier sistema operativo**, Windows incluido.
2. Usa **Git nativo**: no impone un modelo propio de ramas ni un hosting propio.
3. Une **visibilidad, undo y guardrails** en un solo producto.
4. Está **listo para empresa**: integración con **Azure DevOps**, políticas centralizadas y auditoría.

---

## 2. Problema y contexto

| # | Problema | Quién lo sufre | Evidencia / contexto 2026 |
|---|----------|----------------|---------------------------|
| P1 | No hay una vista unificada de qué hace cada agente: rama, worktree, archivos tocados y si sigue corriendo o ya terminó. | Devs y tech leads que usan agentes en paralelo | Auge de orquestadores locales (Conductor, Claude Squad, Superset). Addy Osmani: "3-10 agentes" como caso típico. |
| P2 | Los conflictos entre agentes se descubren tarde, al hacer merge. | Equipos con agentes en paralelo | Claude Code Agent Teams añadió *file locking* experimental, lo que confirma el dolor. |
| P3 | Miedo a acciones destructivas de agentes (reset, force-push, borrado de ramas) y a la *prompt injection*. | Todos los usuarios de agentes | Vulnerabilidades de `mcp-server-git` (ene-2026). Aparecen proxies de políticas (Intercept) y gates (Reflex). |
| P4 | Deshacer el trabajo de un agente es manual: reflog, cherry-pick, adivinar. | Devs junior y semi-senior, y también seniors | Entire Checkpoints y GitButler apuestan por los snapshots y el rewind. |
| P5 | Los agentes generan diffs gigantes, difíciles de revisar e integrar. | Revisores y tech leads | Tendencia hacia PRs apilados y commits pequeños. |
| P6 | En empresa no hay gobierno ni auditoría de qué cambió un agente y bajo qué reglas. | CTO, seguridad, compliance | Entire levantó USD 60M para "agent provenance". Futurum: la procedencia es el activo estratégico. |
| P7 | Las herramientas actuales son **solo Mac** (Conductor), **sin Windows** (Claude Squad usa tmux), **atadas a un agente** o **atadas a GitHub/Linear**. | Equipos corporativos en Windows o con Azure DevOps | Ver sección 3.2. |

---

## 3. Research de mercado

### 3.1 Mapa del mercado

```
                  Visual / GUI
                      ▲
     GitKraken ●      │      ● Conductor (Mac)
     GitLens  ●       │      ● Nimbalyst / Superset
                      │      ● GitButler (agents tab)
 Git clásico ─────────┼──────────────► Agent-aware
                      │      ● Claude Squad (TUI)
     lazygit  ●       │      ● Entire (provenance + hosting)
     Git CLI  ●       │      ● git-mcp / Intercept / Reflex
                      ▼
                  Terminal / headless

  ◎ GitRaptor: agent-aware, terminal + MCP primero y visual después,
    multiplataforma, Git nativo y foco en empresa.
```

### 3.2 Competidores directos (agentes + Git)

| Herramienta | Qué hace | Modelo | Fortalezas | Debilidades / hueco para GitRaptor |
|---|---|---|---|---|
| **Conductor** (Melty Labs) | App que corre agentes Claude Code y Codex en paralelo, cada uno en su worktree, con flujo de diff y PR | Gratis, closed source. Cobrará por colaboración. | UX pulida, revisión de diff y PR | **Solo macOS**. Es un orquestador, no hay guardrails ni undo profundo. |
| **Claude Squad** | TUI en Go: tmux + un worktree por agente | Open source, AGPL-3.0 | Terminal-first, multi-agente | **Sin Windows nativo**. Sin predicción de conflictos, políticas ni undo. AGPL complica el uso empresarial. |
| **GitButler** | Cliente Git con *virtual branches*, pestaña de agentes, CLI `but`, skill y MCP | Freemium | Varios agentes en **un solo directorio**, commit automático por prompt | Te obliga a **adoptar su modelo** (virtual branches). Atado a GitButler como cliente. |
| **Entire** (Thomas Dohmke, ex-CEO de GitHub) | Checkpoints: guarda la sesión del agente en cada commit (rama oculta), shadow branches para rewind, Entire Blame y Review, red Git distribuida | Open source en el CLI. Plataforma y hosting de pago (precios sin anunciar). | Muy bien financiado (USD 60M), visión de procedencia | Va hacia **hosting y plataforma propia**. No es un cockpit local ni tiene guardrails. **Es un posible aliado:** se pueden leer sus checkpoints. |
| **Nimbalyst** (sucesor de Crystal) | App para sesiones paralelas, kanban | Gratis individual. Teams USD 20/usuario/mes. | Kanban y gestión de tareas | Crystal quedó deprecado (feb-2026). Enfoque en tareas, no en Git. |
| **Vibe Kanban** | Kanban en el que cada tarjeta lanza un agente en su rama | Apache-2.0, comunitario | Planificación visual | La empresa cerró (abr-2026). Mantenimiento incierto. |
| **Superset, Parallel Code, Emdash, Sculptor, Abralo** | Orquestadores multi-agente con worktrees | Varios | Multi-agente | Mercado fragmentado y poco diferenciado. Varios rankings los publican los propios competidores. |
| **Cursor Background Agents / Claude Code Agent Teams** | Funciones nativas de cada agente | Incluidas en el agente | Sin fricción | **Cada una atada a su agente.** No dan una vista transversal. |

### 3.3 Competidores adyacentes

| Categoría | Ejemplos | Relación con GitRaptor |
|---|---|---|
| Servidores MCP de Git | `mcp-server-git` (Anthropic), git-mcp (self.agency), GitHub MCP Server | Hacen falta, pero son "Git crudo" con guardrails mínimos. `mcp-server-git` tuvo CVEs en ene-2026. **GitRaptor MCP compite aquí con herramientas de alto nivel y políticas.** |
| Capas de política y seguridad | Intercept (proxy MCP con YAML), Reflex (gate de comandos), GitGuardian MCP | Genéricas, no entienden Git. **GitRaptor aplica políticas con semántica de Git** y puede convivir con ellas. |
| Clientes Git visuales | GitKraken, GitLens, Git Graph (abandonado), Git Graph Plus, GitBit, GitStudio | No pensados para agentes. Fuente de inspiración para la vista visual (Fase 3). Ver Anexo B. |
| Terminal | lazygit, tig, gitui | Referente de UX TUI. Ninguno sabe de agentes. |
| PRs apilados | Graphite, ghstack, git-branchless, Jujutsu (jj, con *operation log* y undo) | Inspiración para Time Machine (oplog de jj) y para Stacks (Fase 3). |

### 3.4 Conclusiones del research

1. **El dolor es real y validado por el mercado:** hay financiamiento (Entire), lanzamientos continuos y funciones nativas en los agentes.
2. **El mercado está fragmentado y es inestable:** Crystal se deprecó y Vibe Kanban cerró. Nadie consolidó todavía la categoría.
3. **Los huecos claros son:**
   - **Multiplataforma real**, con Windows.
   - **Agnóstico del agente.**
   - **Guardrails con semántica de Git.**
   - **Predicción de conflictos entre agentes.**
   - **Foco en empresa con Azure DevOps.**
4. **Riesgo principal:** que las funciones nativas de los agentes (Cursor, Claude Code) o de GitLens 19 absorban parte del valor. Lo mitigamos siendo **transversal** a todos los agentes y **Git nativo**.
5. **No competir con Entire en hosting ni en procedencia.** Mejor **integrarse** con sus checkpoints y estándares abiertos.

---

## 4. Propuesta de valor

> **"Pon a trabajar a 10 agentes en tu repo sin miedo. Ve todo, deshaz todo, y que nadie rompa main."**

| Diferenciador | Conductor | Claude Squad | GitButler | Entire | Funciones nativas del agente | **GitRaptor** |
|---|---|---|---|---|---|---|
| Multiplataforma (Win/Mac/Linux) | ✗ | ✗ (sin Windows) | ✔ | ✔ | ✔ | **✔** |
| Agnóstico del agente | ≈ | ✔ | ≈ | ✔ | ✗ | **✔** |
| Git nativo (sin modelo propio) | ✔ | ✔ | ✗ | ≈ | ✔ | **✔** |
| Cockpit en vivo de agentes | ✔ | ✔ | ✔ | ✗ | ≈ | **✔** |
| Predicción de conflictos entre agentes | ✗ | ✗ | ≈ | ✗ | ≈ | **✔** |
| Undo universal (Time Machine) | ✗ | ✗ | ✔ | ✔ | ≈ | **✔** |
| Guardrails y políticas Git | ✗ | ✗ | ✗ | ✗ | ≈ | **✔** |
| Servidor MCP de alto nivel | ✗ | ✗ | ✔ | ≈ | — | **✔** |
| Azure DevOps y gobierno empresarial | ✗ | ✗ | ✗ | ≈ | ✗ | **✔** |

---

## 5. Usuarios objetivo

| Persona | Descripción | Necesidad principal |
|---|---|---|
| **"Agent wrangler"** | Dev senior que lanza 3-10 agentes en paralelo desde la terminal | Ver todo de un vistazo, saber quién choca con quién, integrar rápido |
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
| BR-11 | **Políticas por repo** en un archivo versionable (`.gitraptor/policy.yaml`): ramas protegidas, prohibir force-push o `reset --hard`, límite de tamaño de diff, formato de commit, rutas prohibidas. | Must |
| BR-12 | **Aplicación de políticas** en dos capas: (a) en las herramientas MCP, que rechazan antes de ejecutar; (b) en los hooks de Git (pre-commit, pre-push, reference-transaction), para cubrir a los agentes que usan Git crudo. | Must |
| BR-13 | **Modo "pedir confirmación":** las acciones de riesgo de un agente quedan en cola para que un humano las apruebe en la TUI. | Should |

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
| BR-23 | **Políticas centralizadas** para la organización: herencia de políticas y distribución desde un repo central. | Should |
| BR-24 | **Auditoría:** exportar el log de acciones de agentes y de violaciones de política (JSON y SIEM). | Should |
| BR-25 | **Dashboard de equipo** (opcional, self-hosted): actividad de agentes por repo y métricas. | Could |

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
| NFR-11 | Legal | Licencia permisiva o open-core apta para empresa (no AGPL en el núcleo). Clean-room respecto a los competidores. |
| NFR-12 | Calidad | Suite de integración contra repos reales con escenarios multi-agente simulados y pruebas de caos (kill del proceso a mitad de una operación). |

---

## 8. Modelo de negocio

| Opción | Descripción | Pros | Contras |
|---|---|---|---|
| **A. Open source total** | Todo MIT o Apache-2.0 y sponsors | Adopción y confianza máximas | Sin ingresos |
| **B. Open-core** (recomendada) | **Gratis:** CLI/TUI, MCP, Time Machine y políticas locales. **De pago (Teams/Enterprise):** políticas centralizadas, auditoría, dashboard, Azure DevOps avanzado y soporte. | La adopción de los devs alimenta la venta a empresa. El comprador (CTO o seguridad) tiene un problema de gobierno real. | Hay que mantener la frontera entre los niveles |
| **C. Herramienta interna** | Uso corporativo (ASSA) | Control total, ajuste a nuestro stack | Sin tracción externa |

**Decisión (v0.3):** opción **C, herramienta interna al inicio**. La arquitectura y la licencia se mantienen compatibles con una futura opción **B (open-core)**, que se reevalúa cuando el MVP esté estable y probado internamente.

---

## 9. KPIs y criterios de éxito

Como el producto arranca como **herramienta interna**, los KPIs miden uso y valor real, no adopción pública:

| KPI | Meta a 6 meses desde el MVP |
|---|---|
| Dogfooding: GitRaptor se usa para construir GitRaptor | 100% de las sesiones de desarrollo con agentes |
| Desarrolladores internos usándolo cada semana | ≥ 5 |
| Repos internos con GitRaptor y `policy.yaml` | ≥ 5 |
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
| Los agentes (Cursor, Claude Code) o GitLens 19 incorporan funciones equivalentes de forma nativa | Alta | Alto | Ser **transversal** a todos los agentes. Los guardrails y la auditoría empresarial difícilmente vienen de un agente en particular. |
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
- **Agentes soportados en el MVP:** **Claude Code y Cursor**. Codex y Copilot pasan a fases posteriores (aplica a BR-02 y BR-15).
- **Hosting:** el repositorio del producto vive en GitHub (`rbonillajr/gitRaptor`). La integración con Azure DevOps (BR-19) se mantiene para los repos internos donde se use la herramienta.

---

## 12. Decisiones y preguntas abiertas

### 12.1 Decisiones tomadas (v0.3)

| # | Pregunta | Decisión |
|---|---|---|
| D1 | ¿Producto público o herramienta interna? | **Herramienta interna al inicio** (opción C), sin cerrar la puerta a open-core (B). |
| D2 | ¿Qué agentes son prioritarios en el MVP? | **Claude Code y Cursor.** Codex y Copilot, después. |
| D3 | ¿Capacidad del equipo? | **Sin equipo humano:** una persona orquestando múltiples agentes de IA. La planificación se hace por historias pequeñas y verificables, no por velocity de un equipo. |

### 12.2 Preguntas abiertas

1. ¿Qué pesa más en el MVP: **Time Machine + Guardrails** (seguridad) o **Cockpit** (visibilidad)? Propuesta: los tres, en versión mínima.
2. ¿Nombre y licencia definitivos? ¿Se mantiene "GitRaptor" y el comando `raptor`?
3. ¿Hay una fecha objetivo para el MVP?
4. ¿Integramos con Entire Checkpoints desde temprano o esperamos a que el estándar madure?

---

## 13. Próximos pasos

1. ~~Validar este documento~~ y cerrar las decisiones principales (hecho en v0.3; quedan las preguntas de 12.2).
2. **Spikes técnicos (1-2 semanas):**
   - (a) Snapshot y undo del working tree con overhead menor a 200 ms.
   - (b) Predicción de conflictos entre N worktrees con `git merge-tree`.
   - (c) Detección de sesiones de Claude Code y Cursor.
   - (d) Prototipo del MCP con una política que bloquee el force-push.
3. **Fase de análisis:** `/aadd-specify` con este BRD → `context.md` → historias de usuario por BR.
4. **Arquitectura:** `/aadd-architect` → overview, ADRs (lenguaje, Git CLI frente a librería, oplog, detección de agentes, modelo de seguridad del MCP) y NFRs técnicos.
5. **Demo "wow" para validar:** 4 agentes en paralelo, el cockpit mostrando un conflicto antes de que ocurra, un agente que intenta hacer force-push y queda bloqueado, y un `raptor undo --agent` que restaura todo.

---

## Anexo A — Fuentes del research (agentes + Git)

- [Addy Osmani — The Code Agent Orchestra](https://addyosmani.com/blog/code-agent-orchestra/) · [Top AI Coding Trends for 2026](https://beyond.addy.ie/2026-trends/)
- [DEV — Best Tools for Managing Parallel AI Coding Agents in 2026](https://dev.to/stravukarl/best-tools-for-managing-parallel-ai-coding-agents-in-2026-14l8) · [Nimbalyst — Agent management tools 2026](https://nimbalyst.com/blog/best-agent-management-tools-2026/) · [AgentsRoom — Multi-agent tools](https://agentsroom.dev/blog/best-multi-agent-coding-tools) · [Parallel Code — Multi-agent tools 2026](https://parallelcode.app/blog/multi-agent-coding-tools-2026/) · [Abralo — alternatives](https://abralo.com/alternatives)
- [Conductor.build intro](https://codepick.dev/en/guides/conductor-build-intro/) · [Superset](https://superset.sh/)
- [GitButler — Agents tab](https://blog.gitbutler.com/agents-tab) · [GitButler 0.16](https://blog.gitbutler.com/gitbutler-0-16) · [GitButler Agent Assist](https://blog.gitbutler.com/gitbutler-agent-assist) · [Parallel Claude Code sin worktrees](https://blog.gitbutler.com/parallel-claude-code) · [gitbutlerapp/claude](https://github.com/gitbutlerapp/claude) · [Trigger.dev — ditched worktrees](https://trigger.dev/blog/parallel-agents-gitbutler)
- [Entire — seed round](https://entire.io/news/former-github-ceo-thomas-dohmke-raises-60-million-seed-round) · [RCP Mag — Entire](https://rcpmag.com/articles/2026/02/12/ex-github-ceo-thomas-dohmke-unveils-entire.aspx) · [OSTechNix — Entire CLI](https://ostechnix.com/entire-cli-git-observability-ai-agents/) · [Futurum — Agent provenance](https://futurumgroup.com/insights/selling-agent-provenance-to-the-cio-entire-changes-who-signs/) · [Entire distributed Git network waitlist](https://windowsforum.com/threads/entire-opens-waitlist-for-distributed-git-network-on-july-8.435946/) · [jd:/dev — Agent-written code needs more than Git](https://julien.danjou.info/blog/github-wont-work-for-ai-agents/) · [Crítica: "nobody asked for"](https://chyshkala.com/blog/github-s-ex-ceo-raises-60m-for-ai-agent-version-control-that-nobody-asked-for)
- [PointGuard AI — mcp-server-git vulnerabilities](https://www.pointguardai.com/ai-security-incidents/git-happens-mcp-flaws-open-door-to-code-execution) · [AgentSeal — mcp-server-git score](https://agentseal.org/mcp/mcp-server-git) · [git-mcp (self.agency)](https://git-mcp.self.agency/) · [github-mcp-server security policy #2136](https://github.com/github/github-mcp-server/issues/2136) · [Reflex MCP guardrails PR](https://github.com/ursuciprian/reflex/pull/73) · [GitGuardian MCP](https://blog.gitguardian.com/shifting-security-left-for-ai-agents-enforcing-ai-generated-code-security-with-gitguardian-mcp/) · [MCP ecosystem 2026](https://codeongrass.com/blog/mcp-server-ecosystem-integration-layer-ai-agents-2026/)

> Nota: varios rankings de herramientas multi-agente los publican competidores (Nimbalyst, AgentsRoom, Abralo, Parallel Code, Superset), así que pueden estar sesgados. Los precios de terceros no están verificados.

## Anexo B — Research previo: extensiones Git para VS Code / Cursor (v0.1)

Se mantiene como insumo para la vista visual de la Fase 3 (BR-22):

- **GitLens** (GitKraken): suite muy completa. Commit Graph, Visual History, Worktrees y AI son **Pro en repos privados**. v17.12 (abr-2026) añadió un sidebar al grafo.
- **Git Graph (mhutchie)**: popular pero **abandonado** desde ~2021, con una licencia que restringe derivados → clean-room obligatorio.
- **Git Graph Plus, GitBit, GitLG, GitStudio**: alternativas activas con pocos usuarios.
- **VS Code nativo**: Source Control Graph (desde v1.93) y worktrees (desde jul-2025), lo que sube el mínimo que hay que ofrecer.
- **Cursor usa Open VSX**: hay que publicar en ambos registros, con el namespace verificado y la verificación de publisher de Cursor.
- **GitKraken Desktop**: referencia de UX (drag & drop, rebase interactivo, undo, workspaces, Launchpad).

Fuentes: [GitLens v17](https://help.gitkraken.com/gitlens/gl-release-v17-x/) · [GitLens Pro](https://gitkraken.com/gitlens/pro-features) · [GitKraken Desktop](https://gitkraken.com/git-client) · [Git Graph #927](https://github.com/mhutchie/vscode-git-graph/issues/927) · [Git Graph Plus](https://open-vsx.org/extension/the0807/git-graph-plus/changes) · [GitBit](https://open-vsx.org/extension/filipstrand/gitbit) · [GitStudio](https://gitstudio.dev/extensions) · [VS Code SCM history](https://code.visualstudio.com/docs/sourcecontrol/history) · [Cursor Extensions](https://cursor.com/help/customization/extensions) · [Open VSX Publishing](https://github.com/eclipse-openvsx/openvsx/wiki/Publishing-Extensions)
