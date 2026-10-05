---
id: RES-GRP-COMP-2026-10
title: "Investigación competitiva 2026-10: GitKraken y los cockpits de agentes, y recomendaciones para que GitRaptor sea competitivo"
type: research
status: proposed
version: 0.1
date: 2026-10-05
author: Rene Bonilla (orquestado por agente)
domain: GRP
related:
  business: [BRD-GRP-001]
  features: [motor-local, cockpit, time-machine, guardrails, mcp]
  adrs: [ADR-CKP-001, ADR-GRD-002, ADR-GRD-005, ADR-GRP-012, ADR-MCP-001, ADR-TMC-001, ADR-TMC-003, ADR-TMC-004]
  stories: [US-TMC-001, US-TMC-002, US-TMC-004, US-TMC-005, US-TMC-009, US-GRD-001, US-GRD-002, US-CKP-001, US-CKP-006, US-CKP-022, US-GRP-007, US-MCP-001]
tags: [competencia, gitkraken, gitlens, kepler, mcp, agent-cockpit, posicionamiento, priorizacion, propuesta]
changelog:
  - 0.1 (2026-10-05): Primera versión. Investigación web con fuentes, matriz de funciones y recomendaciones validadas por PO y Arquitecto.
---

# Investigación competitiva 2026-10: GitKraken y los cockpits de agentes

> **Qué es este documento:** la actualización del análisis competitivo de [BRD-GRP-001](../gitraptor-documento-de-negocio.md) § 3 y Anexo B, más una **propuesta** de priorización para que GitRaptor sea competitivo. Las recomendaciones del § 5 son **propuesta**: no reescriben historias ni el plan de M1 (lo escribe en paralelo el worker `m1-resources`). Los ajustes al backlog se coordinan aparte.
>
> **Método:** investigación web del 2026-10-05 con dos subagentes `internet-researcher`. Cada dato lleva su fuente en el § 7 (`[Gn]` para GitKraken y `[Cn]` para el resto). **NV** = no verificado: la fuente no lo dice o no se pudo abrir la página primaria. Los nombres de productos de terceros se usan solo para identificarlos; GitRaptor no está afiliado a ninguno.

## 1. Resumen ejecutivo

1. **La premisa del BRD v0.6 ya no es cierta.** El BRD dice que GitKraken y GitLens "no están pensados para agentes". Desde **GitKraken Desktop 12.0 (14-abr-2026)** hay una vista **Agents** con una tarjeta por worktree, el estado en vivo de la sesión y un botón para lanzar el agente; desde la **12.4 (5-ago-2026)** se aprueban o deniegan los permisos de Claude Code desde la tarjeta, y en septiembre de 2026 suma estado en vivo de Codex y Copilot CLI [G1][G2][G4]. **GitLens 19.1 (1-sep-2026)** trae lo mismo a VS Code, y GitKraken lanzó **Kepler**, un entorno de desarrollo con agentes en preview gratuita [G15][G17].
2. **"Ver la flota" ya es higiene, no diferencial.** Lo hacen GitKraken, GitLens, Conductor, Nimbalyst, Superset, Claude Code (`claude agents`), Cursor (Agents Window), Codex y Copilot (mission control) [C2][C10][C15][C56][C66][C75][C83].
3. **El hueco que sigue abierto es "proteger y deshacer" de forma transversal.** Ningún actor investigado:
   - **deshace lo que un agente hace con Git crudo** ni el trabajo sin commitear de forma universal: GitKraken deshace solo la última acción hecha en su app [G5]; `/rewind` de Claude Code excluye Bash [C55]; Cursor y Copilot solo restauran archivos [C68][C88]; Codex retiró `/undo` [C76];
   - **aplica políticas con semántica de Git en un servidor MCP**: el MCP de GitKraken expone `git_push` y `git_checkout` sin políticas documentadas [G18][G19]; las allowlists de MCP de GitHub son en la nube y para empresas [C90];
   - **predice conflictos localmente incluyendo trabajo sin commitear**: Conflict Prevention de GitKraken solo mira lo commiteado, exige plan de pago y, entre compañeros, una Org en la nube [G7]. **Clash** (CLI MIT) predice conflictos entre worktrees con un merge simulado y es el único competidor directo de ese pilar [C110];
   - funciona **100 % local, sin cuenta y gratis** con agentes en repos privados: GitKraken exige cuenta y plan Pro para la vista Agents en repos privados [G8][G10].
4. **Hipótesis de diferencial (cambia):** el diferencial de GitRaptor pasa de "ver + deshacer + proteger" a **"proteger y deshacer" por encima de "ver"**. El Cockpit se mantiene en versión mínima y se diferencia por la predicción de conflictos local, no por la vista.

## 2. GitKraken en detalle

### 2.1 GitKraken Desktop

| Función | Qué hace (fuente) | Implicación para GitRaptor |
|---|---|---|
| **Vista Agents** (Agent Sessions View) | Desde la 12.0 (14-abr-2026). Una tarjeta por worktree con rama, cambios sin commitear, ahead/behind y PRs [G1]. Estados documentados: *Running*, *Waiting for input* (campana) y *done*; una "pill" marca el PR mergeado [G2]. Los estados *Thinking* y *Using tool* que ve Rene **no están en la documentación** (NV como nombres oficiales). | La vista de flota de GitRaptor (BR-04) no compite en riqueza visual. |
| **Lanzar agentes** | "+ Start [agente] Session" crea el worktree, corre el setup y lanza el agente [G1][G2]. Agentes que lanza: Claude Code, Codex CLI, Copilot CLI, Cursor CLI, Gemini CLI y OpenCode [G2]. | Lanzar agentes está **fuera de alcance** de GitRaptor (BRD § 6.4). No rehacerlo. |
| **Estado en vivo** | Claude Code (12.0), OpenCode (12.2), Codex (12.4.1, 2-sep-2026), Copilot CLI (12.5, 15-sep-2026) [G1]. Gemini y Cursor solo se lanzan. En la 12.6 el plugin de estado se instala solo si lo pide el usuario [G1]. | Supera a D2 (solo Claude Code en el MVP). Ver R6. |
| **Permisos desde la tarjeta** | Desde la 12.4: los permisos pendientes de Claude Code se aprueban o deniegan con vista previa del comando [G1][G4]. Codex (12.4.1) y Copilot CLI (12.5) también [G1]. | Es la aprobación **del propio agente**, no una política del repo. BR-13 (cola de GitRaptor) aplica reglas de Git que el agente no puede saltarse. |
| **Undo** | Deshace checkout, commit, discard, borrar rama, quitar remoto, reset y rebases [G5]. Solo **la última acción** y solo si GitKraken pudo rastrearla [G5]. En la 12.5 se corrigió un undo de *drop* que **descartaba sin avisar los cambios sin commitear** [G1]. Cobertura de lo hecho por terminal o por un agente con Git crudo: **NV** (no documentado; se infiere que no). | **Hueco principal.** Time Machine observa lo que hace cualquier actor, también por Git crudo, y lo recupera hasta el último estado capturado (US-TMC-004); con los hooks hay punto previo en rebase, ramas y commit (US-TMC-005). |
| **Conflict Prevention** | Detecta ediciones solapadas en cambios **commiteados** no mergeados frente a la rama objetivo y frente a compañeros de la Org [G7]. Plan Pro o superior; entre compañeros exige Org en la nube [G7][G8]. No cruza worktrees o agentes locales entre sí (NV). | GitRaptor predice **localmente**, entre worktrees y con la base, **incluido lo sin commitear** como solape (ADR-CKP-001). Diferencial vigente. |
| **IA** | Mensajes de commit, PR, resolución de conflictos, Commit Composer, explain y code review; por créditos, BYOK, solo de pago [G8][G9]. | No competir. GitRaptor no hace IA generativa en el MVP. |
| **Plataformas y cuenta** | Windows, macOS y Linux [G8]. La versión estándar requiere cuenta (fuente antigua, NV vigencia) [G10]. Sin cuenta ni internet solo con el producto **On-Premise Serverless**, con licencia `.dat` y mínimo 10 usuarios [G8][G11]. | GitRaptor: local, sin cuenta (NFR-03). Diferencial vigente. |
| **Azure DevOps** | Integración en Pro o superior [G13]. Azure DevOps Server en Advanced (inferido, NV). | Azure DevOps **ya no es diferencial por sí solo** (R8). |

### 2.2 Precios de GitKraken

| Plan | Precio | Qué incluye relevante para agentes | Fuente |
|---|---|---|---|
| Free (Community) | $0 | Vista Agents, worktrees y MCP **solo en repos locales y públicos**; sin IA | [G8] |
| Pro (hasta 2 asientos) | ~$10/usuario/mes (**NV oficial**: la página pinta el importe con JS; dato de terceros) | Vista Agents en repos privados, MCP, Launchpad, Conflict detection, IA (1M créditos/semana) | [G8] |
| Advanced (hasta 10) | $14/usuario/mes desde el 2026-07-08 (revendedor) | Integraciones self-hosted y enterprise, SSO, Team Launchpad | [G8][G12] |
| Business (hasta 100) | $216/usuario/año (revendedor) | Control de administración de Conflict Prevention | [G8][G12] |
| Enterprise / On-Premise | $336/usuario/año (revendedor), mínimo 10 | Serverless sin cuenta | [G11][G12] |

> Conclusión: **las funciones de agentes de GitKraken son de pago para repos privados** (Pro o superior). El núcleo de GitRaptor es gratis para cualquier uso interno (D4).

### 2.3 GitLens y Kepler

- **GitLens 18.x:** estado de Claude Code mediante hooks que instala GitLens: *Running*, *Waiting for Input*, *Idle*, *Completed* [G14].
- **GitLens 19.0 (12-ago-2026):** el Commit Graph pasa a ser la vista principal, con *Automatic Rebase*, resolución de conflictos con IA y **undo en un clic** (de la operación del grafo) [G15].
- **GitLens 19.1 (1-sep-2026):** suma Codex, Copilot CLI y OpenCode, y "Start Agent Session" en worktrees [G15]. 19.2 limita el grafo a un worktree; 19.3 (1-oct-2026) abre tareas en Kepler [G15].
- **Pro vs. Community:** Community solo en repos públicos y locales [G16].
- **Kepler** (preview pública gratuita desde el 31-jul-2026): "Agentic Development Environment" con tareas multi-repo y un worktree por repo; soporta Claude Code, Codex, Cursor, Gemini, Copilot, OpenCode, Augment y agentes ACP; por defecto **pide permiso antes de acciones riesgosas**; requiere cuenta [G17]. Es un orquestador: compite con Conductor y Superset, no con Time Machine.

### 2.4 CLI `gk` y servidor MCP de GitKraken

- **22 herramientas** [G18]: Git (`git_add_or_commit`, `git_blame`, `git_branch`, `git_checkout`, `git_log_or_diff`, `git_push`, `git_stash`, `git_status`, `git_worktree`), workspaces y GitLens (`gitkraken_workspace_list`, `gitlens_commit_composer`, `gitlens_launchpad`, `gitlens_start_review`, `gitlens_start_work`), issues y PRs (`issues_*`, `pull_request_*`, `repository_get_file_content`).
- **Cuenta obligatoria** (`gk auth login`); issues, PRs y repos privados en Pro o superior [G18][G19]. Repositorio MIT [G20].
- **Guardrails:** **no documentados** (NV): ni allowlist de repos, ni modo solo lectura, ni confirmación de operaciones destructivas. La única aprobación es la del cliente MCP [G19].
- **`gk work`** (ciclo de un work item multi-repo) y **`gk ai`** (commit, pr, explain, changelog); `gk ai hook` instala hooks de ciclo de vida en Claude Code y OpenCode [G21][G22].

> Implicación: **si** el `git_push` del MCP de GitKraken lanza el Git del sistema, pasa por el hook `pre-push` y la capa de hooks de Guardrails (BR-12 b) lo frena igual. Si usa una librería sin hooks (libgit2, isomorphic-git) o la API del hosting, no. **Qué usa: NV**; se comprueba en el dogfooding (§ 5, R5). GitKraken Desktop ejecuta él mismo un subconjunto de hooks (`pre-commit`, `commit-msg`, `pre-rebase`, `pre-push`, `post-*`) pero **no `reference-transaction`** [G23], así que desde su GUI un force-push se frena pero borrar o mover una rama local no.

## 3. Resto del mercado: quién movió ficha

| Actor | Novedad relevante (2025–2026) | Fuente |
|---|---|---|
| **Conductor** | Ya no solo Claude Code y Codex: también Cursor y OpenCode. **Checkpoint antes de cada turno** en un ref privado, incluido lo sin commitear (Git crudo: NV). MCP alojado sin políticas. Precio: Free, Pro $50/mes, Teams $60/usuario/mes. Sin Windows. | [C1]–[C7] |
| **GitButler** | Pestaña de agentes (0.16), CLI `but` (0.19), `but resolve --ai` (0.22), `but mcp` sin políticas (NV). Oplog con snapshot antes de cada acción mayor, incluido lo sin commitear (Git crudo: NV). Windows sí; Azure DevOps no. FSL-1.1-MIT. Serie A de US$17M (abr-2026). | [C22]–[C32] |
| **Claude Squad** | Sigue igual: TUI con tmux, sin snapshots, sin políticas, sin Windows nativo. AGPL-3.0. | [C8] |
| **Crystal → Nimbalyst** | Crystal deprecado; Nimbalyst: historial de archivos con snapshot antes y después de cada edición de la IA (Git crudo: NV), MCP local sin políticas, Windows sí. MIT; Teams $20/usuario/mes. | [C9]–[C14] |
| **Superset** | IDE de agentes, más de 25 agentes, MCP alojado de ~27 herramientas sin políticas, audit logs en Enterprise. Elastic License 2.0; Pro $20/usuario/mes. Sin Windows. | [C15]–[C17] |
| **Vibe Kanban** | Bloop cerró el 2026-04-10; sigue como proyecto comunitario. Soporta Azure Repos vía `az`. | [C18]–[C21] |
| **Entire** | CLI Checkpoints (MIT): guarda la transcripción junto a cada commit y snapshots sin commitear dentro de sesiones de agente. Sin MCP ni políticas (NV). Windows sí. Precio NV. | [C48]–[C52] |
| **Jujutsu (jj)** | `jj undo` / `op restore`; el Git crudo entra en el op log al importarse, pero solo al correr un comando jj. Sin políticas ni MCP oficial (NV). | [C33]–[C38] |
| **Graphite** | Comprado por Cursor (anunciado 2025-12-19); Cursor pasó a SpaceX (2026-08-14). `gt mcp`. Solo GitHub. | [C39]–[C46] |
| **Claude Code** | `--worktree`/`-w` (v2.1.49), `claude agents` en preview (Working, Needs input, Done), hooks y managed settings. `/rewind` **no cubre Bash ni subagentes**. Sin Azure DevOps en la nube. | [C53]–[C63] |
| **Cursor** | Agentes en paralelo con worktrees (2.0), Agents Window (3.0), checkpoints solo de archivos, hooks con prioridad Enterprise, Azure DevOps en beta. | [C64]–[C72] |
| **Codex** | App con worktrees (Mac, Windows), integrada en ChatGPT; `/undo` eliminado; `codex mcp-server` eliminado; sandbox y hooks gestionados. | [C73]–[C82] |
| **GitHub Copilot** | Agent HQ y mission control; Copilot app GA (17-jun-2026) con un worktree por sesión; atribución fuerte (trailer `Agent-Logs-Url`); `/rewind` solo de archivos; allowlist de MCP para empresas. | [C83]–[C94] |
| **Clash** (nuevo) | CLI MIT en Rust que **predice conflictos entre worktrees** con merge simulado (`clash status`, `watch`). | [C110] |
| **Otros cockpits** | Sculptor, Orca (Stably AI), Warp/Oz, Zed, Amp, ccmanager, Agent of Empires, Container Use (Dagger, con MCP), mux (Coder), Emdash, cmux, opcode, Sidecar. Terragon cerró (2026-02-09). Ninguno con políticas Git ni undo de Git crudo documentado. | [C95]–[C109] |

## 4. Matriz de funciones

Leyenda: ✔ sí · ≈ parcial · ✗ no · NV no verificado · — no aplica. Las celdas de GitRaptor son **compromisos del backlog**, no funciones entregadas (pre-MVP).

### 4.1 Ver, atribuir y predecir

| Herramienta | Ver la flota | Estado de la sesión | Atribución de cambios por agente | Predicción de conflictos |
|---|---|---|---|---|
| **GitRaptor MVP** | ✔ TUI (BR-04, US-CKP-001) | ≈ activa/inactiva/terminada, solo Claude Code (US-GRP-007, D2) | ✔ timeline por actor (BR-10, US-TMC-006) | ✔ local, entre worktrees y con la base; solape incluye sin commitear (BR-06, ADR-CKP-001) |
| **GitRaptor Fase 2/3** | ✔ + vista visual (BR-22) | ✔ más agentes (Codex, Cursor) | ✔ + Entire (BR-20) | ✔ |
| GitKraken Desktop | ✔ | ✔ 4 agentes, con permisos | ≈ por worktree/rama (NV por commit) | ≈ solo commiteado, Pro, nube para compañeros [G7] |
| GitLens | ✔ | ✔ 4 agentes | ≈ (NV) | NV |
| Kepler | ✔ | ✔ | NV | NV |
| Conductor | ✔ | ✔ | ≈ por workspace | NV |
| GitButler | ✔ | ✔ | ✔ por rama virtual | ≈ conflictos de primera clase, predicción NV |
| Claude Squad | ✔ | ≈ | ≈ por worktree | ✗ |
| Nimbalyst | ✔ | ✔ | ✔ archivos por sesión | ✗ |
| Superset | ✔ | ✔ | ≈ | ✗ |
| Entire | ✗ | ✗ | ✔ transcripción por commit | ✗ |
| jj | ✗ | ✗ | ✗ | ✗ (conflictos de primera clase) |
| Claude Code nativo | ✔ `claude agents` (preview) | ✔ | ≈ `agent_id` en hooks | ✗ |
| Cursor nativo | ✔ | ✔ | ≈ Enterprise | ✗ |
| Codex nativo | ✔ | ✔ | NV | ✗ |
| Copilot nativo | ✔ | ✔ | ✔ trailer por commit | ✗ (resuelve después) |
| Clash | ✗ | ✗ | ✗ | ✔ entre worktrees, commiteado (NV sin commitear) |

### 4.2 Proteger y deshacer

| Herramienta | Undo / snapshots | ¿Cubre sin commitear? | ¿Cubre Git crudo del agente? | Guardrails / políticas Git | MCP con política |
|---|---|---|---|---|---|
| **GitRaptor MVP** | ✔ undo, redo, timeline, restaurar (BR-08, BR-09) | ✔ (US-TMC-001, US-TMC-004) | ≈ el **último estado capturado** (US-TMC-004); punto previo por hook solo en rebase, ramas y commit (US-TMC-005, ADR-GRD-002) | ✔ por repo, hooks de Git + MCP (BR-11, BR-12), **para los clientes que ejecutan hooks** | ✔ (BR-14, BR-16) |
| **GitRaptor Fase 2/3** | ✔ | ✔ | ✔ | ✔ + centralizadas (BR-23, edición de equipo) | ✔ + auditoría (BR-24) |
| GitKraken Desktop | ≈ última acción hecha en la app [G5] | ≈ "discard" (NV snapshots) | ✗ (NV; se infiere que no) | ✗ (aprobación del agente, no política) | — |
| GitLens | ≈ undo en el grafo [G15] | NV | ✗ (NV) | ✗ | — |
| `gk` MCP | ✗ | ✗ | ✗ | ✗ (NV) | ✗ expone `git_push` sin políticas (NV) |
| Kepler | NV | NV | NV | ≈ pide permiso antes de acciones riesgosas | NV |
| Conductor | ≈ checkpoint por turno | ✔ | NV | ≈ aprobación por herramienta | ✗ |
| GitButler | ✔ oplog | ✔ | NV | ✗ | ✗ (NV) |
| Claude Squad | ✗ | ✗ | ✗ | ✗ | — |
| Nimbalyst | ≈ historial de archivos | ✔ | NV | ✗ | ✗ |
| Superset | ✗ | ✗ | ✗ | ≈ audit logs (Enterprise) | ✗ |
| Entire | ≈ checkpoints de sesión | ✔ | NV | ✗ (NV) | — |
| jj | ✔ op log | ✔ | ≈ al correr un comando jj | ✗ | — |
| Claude Code nativo | ≈ `/rewind` | ✔ archivos | ✗ excluye Bash [C55] | ≈ hooks y managed settings | — (cliente) |
| Cursor nativo | ≈ checkpoints de archivos | ✔ archivos | ✗ | ≈ hooks Enterprise | ≈ políticas Enterprise |
| Codex nativo | ✗ `/undo` retirado | ✗ | ✗ | ≈ sandbox y hooks | — |
| Copilot nativo | ≈ `/rewind` de archivos | ≈ | ✗ | ≈ hooks y policies en la nube | ≈ allowlist Enterprise (nube) |
| Clash | ✗ | ✗ | ✗ | ✗ | ✗ |

### 4.3 Plataforma y negocio

| Herramienta | Headless / TUI | 100 % local | Windows | Azure DevOps | Precio |
|---|---|---|---|---|---|
| **GitRaptor MVP** | ✔ CLI/TUI + MCP | ✔ sin cuenta (NFR-03) | Objetivo (BR-03); **solo macOS verificado hoy** | ✗ (Fase 2, BR-19) | Núcleo gratis, FSL-1.1-ALv2 (D4) |
| **GitRaptor Fase 2/3** | ✔ + visual | ✔ (dashboard self-hosted) | ✔ | ✔ PRs y work items | Edición de equipo de pago |
| GitKraken Desktop | ✗ GUI | ✗ cuenta; solo Serverless sin cuenta [G11] | ✔ | ✔ Pro+ [G13] | Free (públicos), Pro ~$10, Advanced $14, Business ~$18 |
| GitLens | ✗ (VS Code) | ✗ Pro con cuenta | ✔ | ✔ Pro (NV) | Free (públicos) / Pro |
| `gk` CLI/MCP | ✔ | ✗ cuenta | ✔ | ≈ (NV) | Pro+ para privados |
| Kepler | ✗ | ✗ cuenta | ✔ | NV | Preview gratis |
| Conductor | ≈ CLI/API | NV | ✗ | NV | Free / $50 / $60 |
| GitButler | ✔ `but` | ✔ | ✔ | ✗ | FSL-1.1-MIT |
| Claude Squad | ✔ TUI | ✔ | ✗ | NV | AGPL-3.0 |
| Nimbalyst | ✗ | NV | ✔ | NV | MIT; Teams $20 |
| Superset | ≈ CLI/SDK | ≈ local por defecto | ✗ | NV | ELv2; Pro $20 |
| Entire | ✔ CLI | ≈ nube opcional | ✔ | NV | MIT; NV |
| jj | ✔ | ✔ | ✔ | ✔ como remoto Git | Apache-2.0 |
| Claude Code | ✔ | ✗ (modelo en la nube) | ✔ | ✗ | Propietario |
| Cursor | ✗ | ✗ | ✔ | ≈ beta | Teams $40 |
| Codex | ✔ CLI | ✗ | ✔ | ✗ | Incluido en ChatGPT |
| Copilot | ✔ CLI | ✗ | ✔ | ≈ Boards y revisión en preview | Pro $10, Business $19 |
| Clash | ✔ CLI | ✔ | NV | — | MIT |

## 5. Recomendaciones para ser competitivos (propuesta)

> Decisión del orquestador (2026-10-05), validada por PO y Arquitecto (ver § 6). Son **propuesta**: los cambios al backlog y al plan de M1 se coordinan con el worker `m1-resources` y con Rene; este documento no reescribe historias.

### 5.1 Tesis de posicionamiento (hipótesis)

**R1. "Proteger y deshacer" por encima de "ver".** El Cockpit se mantiene en versión mínima y se diferencia por la predicción de conflictos local, no por la vista. Mensaje, con los ajustes de PO y Arquitecto:

- Decir **"recuperable"**, no "todo reversible": con Git crudo lo recuperable es el **último estado capturado** (riesgo R2 de ADR-TMC-004).
- El diferencial es **local**: el trabajo sin commitear, `reset --hard` y las ramas de los agentes, **antes** del push. Las reglas de rama del hosting ya frenan el force-push en el remoto; "que nadie rompa main" no es el titular.
- Guardrails frena a **las herramientas que ejecutan los hooks de Git**; si alguien los desactiva, el desarrollador se entera (US-GRD-004, US-GRD-012).
- Frase propuesta: *"Tu cliente Git te deja ver a tus agentes. GitRaptor hace recuperable lo que hacen, también con Git crudo y sin commitear, y frena las operaciones de Git peligrosas. Local, sin cuenta y gratis."*
- Esta tesis responde la pregunta abierta 1 del BRD § 12.2. Queda como **propuesta a confirmar por Rene**.

### 5.2 Lista priorizada

| # | Recomendación | Horizonte | Estado frente al backlog |
|---|---|---|---|
| 1 | **Confirmar la tesis R1** (pregunta abierta 1 del BRD). | Ahora | Decide Rene |
| 2 | **M1 demuestra proteger y deshacer:** US-TMC-001, US-TMC-002, US-TMC-004 y US-GRD-001 como Must. **Ya lo recoge el plan de M1 en el PR #62**; esta investigación lo refuerza y pide no sacar ninguna de M1. | M1 | Coincide con el PR #62 |
| 3 | **Decidir antes de la Dev Spec de US-TMC-002 si `raptor undo` actúa sobre el último evento de Git observado** (restaurando el punto de observación anterior) o si eso queda para US-TMC-009. Sin esa decisión, M1 no demuestra "deshacer lo que hizo el agente con Git crudo": en M1 `undo` solo cubre lo que lanza GitRaptor. Propuesta del Arquitecto: enmienda de ADR-TMC-003 § 4. | M1 (bloqueante) | Nueva: enmienda propuesta |
| 4 | **Justo después de M1 (o dentro, si cabe): US-TMC-005 y US-TMC-009.** Subir US-TMC-005 de *medium* a *high* con la promesa **acotada** a rebase, borrado y movimiento de ramas y commit (reset, checkout y merge quedan solo en observación, ADR-GRD-002). US-TMC-009 es la única forma de recuperar lo capturado sin hooks. | M1/M2 | Propuesta de prioridad |
| 5 | **Checklist de convivencia con GitKraken** en el dogfooding de M1 (no una TS: no entrega nada; los casos automatizables van a la regresión de INF-GRD-001). Ver § 5.4. | M1 | Nueva: checklist |
| 6 | **Mantener US-CKP-006** (predicción de conflictos, con el solape de lo sin commitear) en el camino del MVP y **US-CKP-001 mínima**. Es el único diferencial que le queda al Cockpit; vigilar a Clash. | MVP | Sin cambio |
| 7 | **BR-05 / US-CKP-022 (grafo de carriles en la TUI) de Must a Should**, no fuera del MVP. GitKraken y GitLens ya dan un grafo excelente. El PO retoca las menciones al grafo en US-CKP-005 y US-CKP-012. | MVP | Propuesta; decide Rene (prioridad de un BR) |
| 8 | **No rehacer lo que GitKraken ya hace bien:** lanzar agentes (ya fuera de alcance, BRD § 6.4), aprobar los permisos del propio agente, IA generativa (mensajes de commit, PR) y un grafo visual rico en el MVP. | Siempre | Sin cambio |
| 9 | **Push lanzado por otra herramienta:** añadir a INF-GRD-001 un caso "push con Git CLI desde un proceso hijo sin TTY y por stdio" (simula un MCP) → denegado; en US-GRD-001, como **Criterio de Certificación**, no como escenario nuevo (ya tiene 6, el tope). Declarar en la lista publicada de lo no impedible la categoría "cliente sin hooks o API del hosting", y que la explicación de "proteger" recomiende la protección de rama en el servidor. | M1 | Propuesta |
| 10 | **Vigilancia:** repetir esta investigación cada trimestre y con cada versión de GitKraken o Claude Code. Foco: Clash, Kepler, checkpoints de Conductor, Entire, `claude agents` y `/rewind` de Claude Code, oplog de GitButler, y **Git 3.0 con reftable por defecto** (ADR-GRD-002: en repos reftable no se puede impedir renombrar ni reescribir la rama base). | Continuo | Nueva |

### 5.3 Qué no cambia y qué pasa a higiene

- **D2 se mantiene:** solo Claude Code con detección completa en el MVP; luego Codex. La protección es más amplia que la detección, pero no universal: Time Machine **observa** a cualquier actor (su trabajo aparece "sin atribuir") y Guardrails frena a **los clientes que ejecutan hooks de Git**. No decir "cualquier agente, cualquier SO" como promesa del MVP.
- **Windows y Azure DevOps pasan a higiene**, no a titular: siguen siendo requisitos (BR-03 Must, BR-19 Fase 2), pero GitKraken y otros ya los tienen. Riesgo: prometer Windows sin verificarlo; SPIKE-GRD-001 y US-GRD-001 están pendientes en Linux y Windows.
- **La persona Platform/DevEx** pasa a la Fase 2 o a la edición de equipo; el "agent wrangler" que ya usa un cliente Git con vista de agentes y el dev junior con miedo suben de peso.

### 5.4 Convivencia con GitKraken: "GitKraken para ver, GitRaptor para proteger y deshacer"

Checklist propuesto para el dogfooding de M1 (Rene usa GitKraken Desktop a diario):

| # | Comprobación | Qué se espera | Riesgo si falla |
|---|---|---|---|
| K1 | Los snapshots no aparecen en el grafo de GitKraken | Por diseño no hay refs en el repo del usuario: el almacén vive en el perfil (ADR-TMC-001) | Ninguno esperado |
| K2 | Un force-push desde la GUI de GitKraken a un repo protegido | Denegado por `pre-push` | Si GitKraken emula la entrada de `pre-push` distinta a Git, el parseo estricto (ADR-GRD-002 § 4) puede bloquear pushes legítimos (fail-closed) |
| K3 | Borrar o mover una rama local desde la GUI de GitKraken | **No se puede impedir** (no ejecuta `reference-transaction`); queda en observación y se recupera restaurando | Debe estar en la lista publicada de lo no impedible |
| K4 | La preferencia "Git Hooks" de GitKraken reescribe `core.hooksPath` | ADR-GRD-005 lo detecta y avisa de protección inactiva | Protección silenciosamente apagada |
| K5 | Un `git reset --hard` hecho por Claude Code: ¿lo recupera el undo de GitKraken? ¿y GitRaptor? | GitKraken no (verificar, hoy NV); GitRaptor sí, hasta el último estado capturado | Si GitKraken sí lo recupera, el diferencial se achica: corregir el mensaje |
| K6 | ¿El `git_push` del MCP de GitKraken lanza el Git del sistema? (`GIT_TRACE=1` o `execsnoop`) | Si lo lanza, `pre-push` lo frena | Si no, se declara como cliente sin hooks |
| K7 | Ruido del auto-fetch y del refresco del índice de GitKraken | Sin capturas inútiles; CPU en reposo < 1 % (RES-01) | Propuesta del Arquitecto: filtrar los disparadores de captura en eventos que solo tocan `refs/remotes`/`FETCH_HEAD` y añadir al banco de INF-GRP-002 un escenario con fetch periódico |
| K8 | Contención de `index.lock` entre GitKraken y el aplicador o el ejecutor | Reintento o rechazo limpio (US-TMC-015) | Operación a medias |
| K9 | Un Claude Code lanzado desde "Start Claude Code Session" de GitKraken | US-GRP-007 lo detecta (por proceso, cwd y transcripts, ADR-GRP-012) | La detección por ruta del ejecutable puede no reconocer un envoltorio |
| K10 | Los `git` que lanzan los hooks de Claude Code de GitKraken (plugin de estado, `gk ai hook`) | Se atribuyen a Claude Code por ascendencia; aceptable y documentado | Atribución confusa |
| K11 | Worktrees creados por GitKraken | El motor los ve (registro por `gitdir`) y `core.hooksPath` absoluto los cubre | Ubicación fuera del repo: NV, sin impacto esperado |

GitRaptor **no escribe** en la configuración de Claude Code (`~/.claude/settings.json`): detecta sesiones sin hooks de Claude Code (ADR-GRP-012) y el MCP se instala con `claude mcp add` (ADR-MCP-001 § 8). No compite con los hooks del plugin de estado de GitKraken.

### 5.5 Propuestas al backlog, BRD y ADRs (no aplicadas)

| ID | Propuesta | Dueño | Decide |
|---|---|---|---|
| P-01 | M1 con US-TMC-001, 002, 004 y US-GRD-001 como Must | Worker `m1-resources` (PR #62) | Ya en el PR #62 |
| P-02 | US-TMC-005 de *medium* a *high*, con la promesa acotada; cambiar el ejemplo del escenario 1 (checkout) por un rebase o un borrado de rama | PO | Rene |
| P-03 | US-TMC-009 justo después de M1, o dentro si la enmienda de ADR-TMC-003 lo exige | PO + `m1-resources` | Rene |
| P-04 | BR-05 / US-CKP-022 de Must a Should | PO | Rene |
| P-05 | Checklist de convivencia K1–K11 en el dogfooding de M1; casos automatizables en INF-GRD-001 | `m1-resources` + INF-GRD-001 | Orquestador |
| P-06 | Criterio de Certificación en US-GRD-001: push desde un proceso hijo sin TTY (simula un MCP) denegado | PO | Orquestador |
| P-07 | Enmienda a ADR-GRD-002 § 2–3: saltos declarados nuevos (clientes sin hooks o con un subconjunto, como libgit2, go-git o GitKraken Desktop sin `reference-transaction`; escrituras por la API del hosting) | Arquitecto | Orquestador |
| P-08 | Enmienda a ADR-TMC-004 § 3: lista cerrada de hooks que producen `previo_hook`; reset, checkout y merge solo en observación | Arquitecto | Orquestador |
| P-09 | Enmienda a ADR-TMC-003 § 4: ¿`raptor undo` actúa sobre el último evento observado? Afecta a US-TMC-002, 003, 010 y 011 | Arquitecto + PO | Rene (cambia la promesa de M1) |
| P-10 | BRD § 9: añadir KPIs "tiempo para recuperarse de un error (s)", "recuperaciones de trabajo hecho con Git crudo" y "0 interferencias con GitKraken durante el dogfooding"; pasar el "−30 % al integrar" a secundario | PO | Rene |
| P-11 | BRD § 13: nueva demo sin US-TMC-011 (bloqueada por P17): GitKraken abierto mostrando los agentes; un agente hace `git reset --hard` y GitRaptor lo recupera; un force-push queda bloqueado; un solape sin commitear se ve antes del merge | PO | Rene |
| P-12 | Filtro de disparadores de captura para `refs/remotes`/`FETCH_HEAD` y escenario de fetch periódico en INF-GRP-002 | Arquitecto | Orquestador |

## 6. Validación con PO y Arquitecto

**Decisión del orquestador (2026-10-05), validada por PO y Arquitecto.** Las dos validaciones se hicieron por separado sobre el mismo borrador (R1–R10, P-01–P-05) y sin editar archivos.

| Borrador | PO | Arquitecto | Resultado |
|---|---|---|---|
| R1 tesis | Con ajuste: "recuperable", diferencial local, "si desactivan los hooks, te enteras"; decide Rene | Con ajuste: "recuperable", no "no pueden romper main" | § 5.1 con los dos ajustes; pendiente de Rene |
| R2 / P-01 M1 | Con ajuste: undo en M1 solo de lo que lanza GitRaptor | Aprobada; ya en el PR #62; faltan Dev Specs y la regla base de US-TMC-013 | § 5.2 #2 y #3 |
| R3 / P-02 TMC-004/005 tras M1 | Con ajuste: sumar US-TMC-009, subir TMC-005 | Rechazada tal como estaba: TMC-004 ya es Must en M1; TMC-005 solo cubre rebase, ramas y commit | **Discrepancia aparente resuelta con datos:** el PO no tenía el PR #62. TMC-004 se queda en M1; TMC-005 y TMC-009 después, con la promesa acotada (§ 5.2 #4) |
| R4 / P-03 grafo | Aprobada; a Should, no post-MVP | Aprobada | § 5.2 #7 |
| R5 / P-04 convivencia | Con ajuste: checklist + criterios, verificar el undo de GitKraken | Con ajuste: checklist, no TS; (a) sobra por diseño; riesgos de hooks | § 5.4 (K1–K11) |
| R6 agentes | Con ajuste: protección para quien usa Git CLI, "sin atribuir" | Con ajuste: "por construcción" solo en la observación | § 5.3 |
| R7 / P-05 MCP | Con ajuste: criterio de certificación, no escenario | Con ajuste: sin nombre de producto; NV qué usa el MCP de GitKraken | § 5.2 #9, § 2.4 corregido a NV |
| R8 higiene | Aprobada; Platform/DevEx a Fase 2 | Aprobada; riesgo de prometer Windows | § 5.3 |
| R9 vigilancia | Con ajuste: Conductor, Entire, `/rewind`; también por versión | Aprobada; añadir Git 3.0 reftable | § 5.2 #10 |
| R10 BRD | Con ajuste: más secciones (§ 5, § 9, § 12.2, § 13) | Aprobada; corregir la matriz | BRD v0.7; § 9 y § 13 quedan como propuesta (P-10, P-11) porque cambian KPIs y la demo |

**Matriz corregida por el Arquitecto:** en la fila de GitRaptor MVP, "Git crudo del agente" pasa de ✔ a ≈ y Guardrails se limita a "los clientes que ejecutan hooks" (§ 4.2).

**Sin discrepancias abiertas.** Ninguna recomendación elimina un BR ni sale del alcance del MVP. Las decisiones que quedan para Rene son: la tesis (pregunta abierta 1), la prioridad de BR-05, la promesa de undo en M1 (P-09), los KPIs (P-10) y la demo (P-11).

## 7. Fuentes

Todas consultadas el **2026-10-05**. Entre paréntesis, la fecha del contenido cuando se conoce.

### GitKraken (`[Gn]`)

1. [GitKraken Desktop Release Notes](https://help.gitkraken.com/gitkraken-desktop/current/) (12.0, 2026-04-14 a 12.5, 2026-09-15; 12.6 sin fecha)
2. [Coding Agents in GitKraken Desktop](https://help.gitkraken.com/gitkraken-desktop/agents/) (fecha NV)
3. [GitKraken blog — Desktop 12.0 Agent Mode](https://www.gitkraken.com/blog/youre-running-agents-your-tooling-is-still-catching-up) (abr-2026)
4. [GitKraken blog — Desktop 12.4: see every agent, approve every change](https://gitkraken.com/blog/gitkraken-desktop-12-4-see-every-agent-approve-every-change) (2026-08-05)
5. [Undo and Redo](https://help.gitkraken.com/gitkraken-desktop/undo-and-redo/) (mar-2026)
6. [GitKraken blog — Desktop 11.8](https://gitkraken.com/blog/gitkraken-desktop-11-8-visibility-where-it-matters-undo-when-it-doesnt) (fecha NV)
7. [Conflict Prevention](https://help.gitkraken.com/gitkraken-desktop/conflict-prevention/) (abr-2026)
8. [GitKraken Pricing](https://www.gitkraken.com/pricing) (vigente; importes renderizados con JS, no extraídos)
9. [GitKraken AI FAQ](https://help.gitkraken.com/gitkraken-desktop/gitkraken-ai/) (fecha NV)
10. [Feedback — Allow users to sign out](https://feedback.gitkraken.com/suggestions/212373/allow-users-to-sign-out) (antigua, fecha NV)
11. [On-Premise Serverless](https://help.gitkraken.com/gitkraken-desktop/serverless/) (sep-2026)
12. [schneider.im — GitKraken new list prices](https://www.schneider.im/gitkraken-new-list-prices-for-new-sales-and-renewals/) (revendedor, 2026-07-20)
13. [Azure DevOps Integration](https://help.gitkraken.com/gitkraken-desktop/azure-devops/) (mar-2026)
14. [AI Agents in GitLens](https://help.gitkraken.com/gitlens/gl-agents/) (may-2026)
15. [GitLens Release Notes](https://help.gitkraken.com/gitlens/gitlens-release-notes-current/) (2026-08-12 a 2026-10-01)
16. [GitLens Docs](https://help.gitkraken.com/gitlens/gitlens-home/) (ago-2026)
17. [GitKraken blog — Kepler public preview](https://gitkraken.com/blog/kepler-is-in-public-preview-one-task-every-repo-every-agent) (2026-07-31)
18. [MCP Tools Reference](https://help.gitkraken.com/mcp/mcp-tools-reference/) (fecha NV)
19. [Getting Started with the GitKraken MCP Server](https://help.gitkraken.com/mcp/mcp-getting-started/) (mar-2026)
20. [github.com/gitkraken/mcp](https://github.com/gitkraken/mcp)
21. [GitKraken blog — AI-focused CLI commands](https://gitkraken.com/blog/save-time-with-our-new-ai-focused-cli-commands) y [gk work items](https://help.gitkraken.com/cli/gk-cli-work-items/) (vía resumen de búsqueda; páginas primarias no abiertas, NV)
22. [Install and Set Up GitKraken CLI](https://help.gitkraken.com/gitkraken-cli/cli-home/) (mar-2026)
23. [Git Hooks en GitKraken Desktop](https://help.gitkraken.com/gitkraken-desktop/githooks/) y [notas de la 7.x (`core.hooksPath`)](https://help.gitkraken.com/gitkraken-desktop/7x/) (aportadas por el Arquitecto en la validación)
24. [libgit2 #964: sin soporte de hooks](https://github.com/libgit2/libgit2/issues/964) · [githooks de Git](https://git-scm.com/docs/githooks)

### Resto del mercado (`[Cn]`)

Algunos datos vienen de resúmenes del buscador cuando la página primaria no se pudo abrir; en la tabla del § 3 esos datos van como NV.

1. [conductor.build/docs](https://conductor.build/docs) · 2. [Conductor changelog](https://conductor.build/changelog) · 3. [Conductor checkpoints](https://conductor.build/docs/core/checkpoints) · 4. [Conductor security and permissions](https://conductor.build/docs/reference/security-and-permissions) · 5. [Conductor MCP](https://conductor.build/docs/api/mcp) · 6. [Conductor pricing](https://conductor.build/pricing) · 7. [Conductor installation](https://conductor.build/docs/installation) (sin Windows, resumen de búsqueda)
8. [smtg-ai/claude-squad](https://github.com/smtg-ai/claude-squad) (v1.0.20, 2026-08-20)
9. [stravu/crystal](https://github.com/stravu/crystal) · 10. [nimbalyst.com](https://nimbalyst.com) · 11. [Nimbalyst worktrees](https://nimbalyst.com/docs/developer-features/worktrees/) · 12. [Nimbalyst file history](https://nimbalyst.com/docs/file-management/file-history-and-restore/) · 13. [Nimbalyst MCP](https://nimbalyst.com/docs/setup-nimbalyst/mcp/) · 14. [Nimbalyst pricing](https://nimbalyst.com/pricing/)
15. [superset.sh](https://superset.sh) y [superset-sh/superset](https://github.com/superset-sh/superset) · 16. [Superset MCP](https://docs.superset.sh/mcp-server) · 17. [Superset pricing](https://superset.sh/pricing)
18. [Vibe Kanban — shutdown](https://vibekanban.com/blog/shutdown) · 19. [BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban) · 20. [Vibe Kanban MCP](https://vibekanban.com/docs/integrations/vibe-kanban-mcp-server) · 21. [Vibe Kanban Azure Repos](https://vibekanban.com/docs/integrations/azure-repos-integration)
22. [GitButler — Agents tab](https://blog.gitbutler.com/agents-tab) · 23. [GitButler 0.16](https://blog.gitbutler.com/gitbutler-0-16) · 24. [gitbutlerapp/claude](https://github.com/gitbutlerapp/claude) · 25. [GitButler 0.19](https://blog.gitbutler.com/gitbutler-0-19) · 26. [GitButler 0.22](https://blog.gitbutler.com/gitbutler-0-22) · 27. [GitButler timeline](https://docs.gitbutler.com/features/timeline) · 28. [Using the GitButler MCP](https://blog.gitbutler.com/using-gb-mcp) · 29. [GitButler for Windows](https://blog.gitbutler.com/gitbutler-for-windows) · 30. [gitbutler#2651 (Azure DevOps)](https://github.com/gitbutlerapp/gitbutler/issues/2651) · 31. [GitButler LICENSE](https://raw.githubusercontent.com/gitbutlerapp/gitbutler/master/LICENSE.md) · 32. Pulse 2.0 — GitButler Series A de US$17M (abr-2026)
33. [jj operation log](https://docs.jj-vcs.dev/latest/operation-log/) · 34. [jj working copy](https://docs.jj-vcs.dev/latest/working-copy/) · 35. [jj Git compatibility](https://docs.jj-vcs.dev/latest/git-compatibility/) · 36. [jj-agentic-workflow](https://github.com/CodeAlive-AI/jj-agentic-workflow) · 37. [claude-jj-worktree](https://github.com/jasagiri/claude-jj-worktree) · 38. [jj-vcs/jj](https://github.com/jj-vcs/jj)
39. [Cursor blog — Graphite](https://cursor.com/blog/graphite) · 40. [Graphite joins Cursor](https://graphite.com/blog/graphite-joins-cursor) · 41. Fortune (2025-12-19) · 42. The Next Web — SpaceX completa la compra de Cursor (2026-08-14) · 43. [Graphite Agent and pricing](https://graphite.com/blog/introducing-graphite-agent-and-pricing) · 44. [Graphite — Cursor cloud agents](https://graphite.com/blog/cursor-cloud-agents) · 45. [Graphite — Claude and `gt`](https://graphite.com/blog/how-i-got-claude-to-write-better-code) · 46. [Graphite pricing](https://graphite.com/pricing) · 47. [GitHub stacked PRs public preview](https://github.blog/changelog/2026-07-30-stacked-pull-requests-are-now-in-public-preview/) (2026-07-30)
48. TechCrunch — Entire, ronda de US$60M (2026-02-10) · 49. [Entire — seed round](https://entire.io/news/former-github-ceo-thomas-dohmke-raises-60-million-seed-round/) · 50. [entireio/cli](https://github.com/entireio/cli) · 51. [entireio/cli CHANGELOG](https://github.com/entireio/cli/blob/main/CHANGELOG.md) · 52. GeekWire — Entire (2026)
53. [Claude Code CHANGELOG](https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md) · 54. [Common workflows](https://code.claude.com/docs/en/common-workflows) · 55. [Checkpointing](https://code.claude.com/docs/en/checkpointing) · 56. [Agent view](https://code.claude.com/docs/en/agent-view) · 57. [Desktop](https://code.claude.com/docs/en/desktop) · 58. [Agent teams](https://code.claude.com/docs/en/agent-teams) · 59. [Hooks](https://code.claude.com/docs/en/hooks) · 60. [Settings](https://code.claude.com/docs/en/settings) · 61. [Claude Code on the web](https://code.claude.com/docs/en/claude-code-on-the-web) · 62. [Setup](https://code.claude.com/docs/en/setup) · 63. [Claude Code LICENSE](https://github.com/anthropics/claude-code/blob/main/LICENSE.md)
64. [Cursor 2.0](https://cursor.com/changelog/2-0) · 65. [Cursor 3.0](https://cursor.com/changelog/3-0) · 66. [Agents Window](https://cursor.com/docs/agent/agents-window) · 67. [Cursor worktrees](https://cursor.com/docs/configuration/worktrees) · 68. [Cursor checkpoints](https://cursor.com/docs/agent/chat/checkpoints) · 69. [Cursor hooks](https://cursor.com/docs/agent/hooks) · 70. [Cursor Azure DevOps](https://cursor.com/docs/integrations/azure-devops) · 71. [Cursor pricing](https://cursor.com/pricing) · 72. [Foro de Cursor — conflictos multi-agente](https://forum.cursor.com/t/wrong-merge-conflicts-possibly-only-in-multi-agent/146203)
73. OpenAI — Introducing the Codex app (resumen de búsqueda) · 74. [ChatGPT what's new](https://learn.chatgpt.com/docs/whats-new) · 75. [Codex worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees) · 76. [openai/codex#9618 (`/undo` retirado)](https://github.com/openai/codex/discussions/9618) · 77. [Codex approvals and security](https://developers.openai.com/codex/agent-approvals-security) · 78. [Codex hooks](https://developers.openai.com/codex/hooks) · 79. [Codex MCP server](https://learn.chatgpt.com/docs/mcp-server) · 80. [openai/codex](https://github.com/openai/codex) · 81. [openai/codex#10665 (Azure DevOps)](https://github.com/openai/codex/issues/10665) · 82. [Codex pricing](https://learn.chatgpt.com/docs/pricing)
83. [GitHub — Welcome home, agents](https://github.blog/news-insights/company-news/welcome-home-agents/) · 84. [Agent HQ — Claude y Codex](https://github.blog/news-insights/company-news/pick-your-agent-use-claude-and-codex-on-agent-hq/) · 85. [Copilot app GA](https://github.blog/changelog/2026-06-17-github-copilot-app-generally-available/) (2026-06-17) · 86. [Trace Copilot commits to session logs](https://github.blog/changelog/2026-03-20-trace-any-copilot-coding-agent-commit-to-its-session-logs/) · 87. [Copilot CLI GA](https://github.blog/changelog/2026-02-25-github-copilot-cli-is-now-generally-available/) · 88. [Copilot CLI roll back changes](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/roll-back-changes) · 89. [Copilot CLI hooks](https://docs.github.com/en/copilot/tutorials/copilot-cli-hooks) · 90. [Enterprise MCP allowlist](https://docs.github.com/en/enterprise-cloud@latest/copilot/how-tos/administer-copilot/manage-mcp-usage/configure-enterprise-allowlist) · 91. [Copilot for Azure Boards](https://devblogs.microsoft.com/devops/github-copilot-for-azure-boards/) · 92. [Copilot code review for Azure Repos](https://github.blog/changelog/2026-06-02-github-copilot-code-review-for-azure-repos-is-now-in-technical-preview/) · 93. [Copilot usage-based billing](https://github.blog/news-insights/company-news/github-copilot-is-moving-to-usage-based-billing/) · 94. [Copilot coding agent GA](https://github.blog/changelog/2025-09-25-copilot-coding-agent-is-now-generally-available/)
95. [Imbue — Sculptor](https://imbue.com/blog/sculptor-announce) · 96. [terragon-labs/terragon-oss](https://github.com/terragon-labs/terragon-oss) · 97. [stablyai/orca](https://github.com/stablyai/orca) · 98. [Warp — multi-harness orchestration](https://warp.dev/blog/multi-harness-cloud-agent-orchestration) · 99. [Zed — parallel agents](https://zed.dev/blog/parallel-agents) · 100. releasebot.io — Amp (terceros) · 101. [devflowinc/uzi](https://github.com/devflowinc/uzi) · 102. [kbwo/ccmanager](https://github.com/kbwo/ccmanager) · 103. Better Stack — Agent of Empires · 104. [dagger/container-use](https://github.com/dagger/container-use) · 105. [coder/mux](https://github.com/coder/mux) · 106. [generalaction/emdash](https://github.com/generalaction/emdash) · 107. [manaflow-ai/cmux](https://github.com/manaflow-ai/cmux) · 108. [winfunc/opcode](https://github.com/winfunc/opcode) · 109. [marcus/sidecar](https://github.com/marcus/sidecar) · 110. [clash-sh/clash](https://github.com/clash-sh/clash)
