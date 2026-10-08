---
id: DS-US-MCP-001
title: "Dev Spec — US-MCP-001: servidor MCP por stdio e instalación en Claude Code"
type: dev-spec
status: implemented
feature: mcp
domain: MCP
created: 2026-10-05
updated: 2026-10-08
related:
  stories: [US-MCP-001]
  adrs: [ADR-MCP-001, ADR-GRP-001, ADR-GRP-005]
  rules: [BR-MCP-WF-007, BR-MCP-EDGE-009, BR-MCP-TIME-003]
  nfrs: [NFR-02, NFR-03, NFR-10, SEC-14, SEC-MCP-07, SEC-MCP-08, SEC-MCP-09, SEC-MCP-10]
tags: [mcp, rmcp, stdio, instalacion, claude-code, s-mcp-3, s-mcp-5, ola-1]
---

# Dev Spec — US-MCP-001: servidor MCP por stdio e instalación en Claude Code

Plano compacto (AADD ligero) de [US-MCP-001](../user-stories/US-MCP-001-instalar-en-claude-code.md). El contrato lo fija [ADR-MCP-001](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md) § 1 (proceso y transporte), § 8 (instalación) y § 9 (MCP09). Las herramientas son de US-MCP-003 en adelante.

## 1. Decisiones

Todas son **Decisión del orquestador (2026-10-04), validada por Arquitecto** (técnica) **y PO** (alcance). La columna de la derecha recoge los ajustes que pidieron y que ya están incorporados.

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D1 | `raptor-mcp` **no** conecta ni arranca el daemon al iniciar (ADR-MCP-001 § 1, BR-MCP-TIME-003). Sin herramientas no hay "primera llamada": la conexión perezosa del canal llega con la primera herramienta (US-MCP-003). El test de TS-GRP-004 que pedía lo contrario (`apps/mcp/tests/on_demand.rs`) se sustituye por `handshake.rs::the_handshake_does_not_start_the_engine`. Se quita la dependencia sin uso de `gitraptor-policy`. | Arquitecto: de acuerdo; además, el health check de `claude mcp get` lanza el servidor y así no arranca el motor. El arranque bajo demanda de la biblioteca cliente sigue cubierto por `apps/cli/tests/channel_process.rs`; el del perfil `mcp` vuelve con US-MCP-003. |
| D2 | `rmcp` 3.5.0 con `default-features = false` y solo `server` y `transport-io`; tokio de un hilo (`rt`, `io-std`, `time`). Capabilities: solo `tools` con `listChanged: false` **explícito**; cero herramientas; `serverInfo` `{name: "gitraptor", version}` e `instructions` constantes en inglés. stdout solo protocolo; stderr solo códigos fijos (`handshake-failed`, `transport-failed`, `runtime-unavailable`, `internal-error`); *panic hook* que no imprime el mensaje. Nunca `chdir`. | Arquitecto: fijar `listChanged: false` explícito y probar la respuesta exacta de `initialize`. `cargo-deny` para las features de red queda para SEC-07 / US-MCP-003. |
| D3 | `raptor mcp install\|uninstall [--agent …]` en `apps/cli`. `claude` se resuelve en el `PATH` (solo carpetas absolutas), canonicalizado, archivo regular no escribible por grupo u otros; se ejecuta con argv fijo, sin shell, sin stdin y con cwd `/`, para que `claude mcp get` no lea un `.mcp.json` de proyecto ni el ámbito local de la carpeta. | Arquitecto: registrar la ruta **absoluta sin canonicalizar** de `raptor-mcp` junto a `raptor` (un symlink de Homebrew sobrevive a `brew upgrade`) y comparar la propiedad **canonicalizando ambos lados**; exigir además transporte stdio, sin args y sin entorno. El entorno se hereda (lo necesitan `HOME`, `CLAUDE_CONFIG_DIR` y el `PATH` de node). |
| D4 | Propiedad: es "nuestro" solo si `claude mcp get gitraptor` lo muestra en ámbito de usuario, stdio, con `Command` igual (canónico) al `raptor-mcp` instalado, sin `Args` ni `Environment`. Cualquier otro `gitraptor` (otro binario, otros args, otro ámbito) es ajeno: no se sobrescribe ni se retira (BR-MCP-EDGE-009). | Riesgo residual aceptado: `get` hace un health check que **ejecuta** el `gitraptor` ajeno; Claude Code ya lo ejecuta en cada sesión, así que la exposición marginal es mínima. |
| D5 | Binario instalado (SEC-14): rechazo si la ruta (cruda o canónica) tiene un componente `_npx`, `pnpm…/dlx`, `bunx-…`, o cuelga de una carpeta temporal (`temp_dir()`, `/tmp`, `/private/tmp`, `/var/tmp`, `/var/folders`, `/private/var/folders`); `raptor-mcp` debe existir junto a `raptor`, regular y no escribible por grupo u otros. | Arquitecto: comprobar la ruta cruda y la canónica; añadir pnpm `dlx` y `bunx`. `TMPDIR` lo controla cualquiera: en el peor caso da un rechazo falso. |
| D6 | Sin confirmación interactiva: `install` muestra el comando exacto y su efecto, y lo ejecuta. Sin la CLI `claude`, imprime el comando (entrecomillado para shell POSIX) en stdout, la explicación en stderr y sale con código 1. Una respuesta de `claude` con otra forma (S-MCP-5) no cambia nada y dice "actualiza GitRaptor o ejecuta tú este comando". | PO: de acuerdo (BR-MCP-WF-007 dice "muestra", no "pregunta"; salir con 0 haría creer a un script que se instaló). |
| D7 | `--agent` acepta cualquier texto: `claude-code` (por defecto); `cursor`, `codex`, `copilot` y cualquier otro valor → "`<Agente>` no está soportado todavía; usa --agent claude-code", sin error del parser. | PO: todo rechazo lleva motivo **y** acción; el conflicto dice "retíralo con claude mcp remove gitraptor y vuelve a instalar". |
| D8 | `uninstall`: sin `gitraptor` → "nada que hacer" (código 0); ajeno → rechazo sin cambios con la acción `claude mcp remove gitraptor`. | PO: de acuerdo. |
| D9 | Fuera de alcance, anotado en el PR: el diagnóstico de `raptor doctor` para un `gitraptor` en un `.mcp.json` de proyecto (SEC-MCP-10; `doctor` no existe todavía), ofrecer añadir el repo a la allowlist (US-MCP-002) y las herramientas (US-MCP-003+). | Arquitecto: ADR-MCP-001 § 9 asigna ese diagnóstico a US-MCP-001, así que queda explícito como **pendiente** con dueño: la historia que cree `raptor doctor`. |

## 2. Supuestos comprobados

**S-MCP-5 (forma de la CLI `claude`)**, Claude Code 2.1.284 en macOS, con `CLAUDE_CONFIG_DIR` temporal:

- `claude mcp add --scope user --transport stdio gitraptor -- <ruta>` escribe `mcpServers.gitraptor = {type: "stdio", command, args: [], env: {}}` en el `.claude.json` del directorio de configuración; repetirlo falla con "already exists".
- `claude mcp get gitraptor` imprime texto (sin `--json`): `Scope`, `Status`, `Type`, `Command`, `Args`, `Environment`; sale con 1 y "No MCP server named" si no existe. **Lanza el servidor** como health check: con el `raptor-mcp` real responde `✔ Connected`.
- Desde una carpeta con `.mcp.json`, `get` devuelve la entrada de proyecto (`Scope: Project config`, sin `Command`): por eso se ejecuta desde `/`.
- `claude mcp remove --scope user gitraptor` la retira y deja las demás.

Las respuestas reales están en `apps/cli/tests/fixtures/claude-2.1.284/` y el parser se prueba contra ellas en cada build.

**S-MCP-3 repetida con `--scope user`** (ADR-MCP-001, Validación 1): un servidor de sonda registrado con ámbito de usuario y lanzado por el health check de `claude mcp get` desde `proj/sub` arrancó con cwd `proj/sub`, padre directo `claude` y `CLAUDE_PROJECT_DIR=proj/sub`, igual que con `--mcp-config`. Es el mismo mecanismo de lanzamiento, no una sesión completa (`claude -p` necesita login, que el directorio temporal no tiene).

## 3. Código

| Pieza | Ubicación |
|---|---|
| Servidor (`ServerHandler` con `get_info` constante) | `apps/mcp/src/server.rs` |
| Proceso: runtime, stdio, códigos de stderr, *panic hook* | `apps/mcp/src/main.rs` |
| `raptor mcp install\|uninstall`, parser de `claude mcp get`, reglas de propiedad y de binario instalado | `apps/cli/src/mcp.rs` |
| Mensajes en/es | `apps/cli/i18n/{en,es}.txt` (`mcp.*`) |

## 4. Pruebas

| Escenario o regla | Prueba |
|---|---|
| Instala con ámbito de usuario, informa antes, otros servidores iguales, ningún archivo del repo cambia, nada en la allowlist (sin perfil) | `apps/cli/tests/mcp_install.rs::install_registers_gitraptor_in_the_user_scope` (argv exacto y cwd `/`) |
| Instalar dos veces no cambia nada | `mcp_install.rs::installing_twice_changes_nothing` |
| Retirar el servidor, los otros dos siguen | `mcp_install.rs::uninstall_removes_only_gitraptor` |
| Esquema: Cursor, Codex, Copilot | `mcp_install.rs::unsupported_agents_are_refused` |
| Esquema: caché de npx o carpeta temporal | `mcp_install.rs::a_binary_in_a_temporary_folder_is_refused`; `mcp::tests::volatile_locations` |
| Esquema: `gitraptor` ajeno | `mcp_install.rs::a_foreign_gitraptor_is_never_overwritten`, `our_path_with_extra_args_is_foreign` |
| Sin la CLI de Claude Code, comando exacto | `mcp_install.rs::without_claude_the_exact_command_is_shown`, `a_group_writable_claude_is_not_trusted` |
| S-MCP-5: forma desconocida | `mcp_install.rs::an_unknown_claude_answer_changes_nothing`; `mcp::tests::*` contra las respuestas reales |
| i18n | `mcp_install.rs::messages_in_spanish`; `i18n::tests::catalogs_match` |
| Handshake MCP por stdio, `tools/list` vacío, cierre de stdin | `apps/mcp/tests/handshake.rs::completes_the_mcp_handshake_over_stdio_and_exits_when_stdin_closes` |
| BR-MCP-TIME-003: el handshake no arranca el motor | `handshake.rs::the_handshake_does_not_start_the_engine` |
| SEC-MCP-07: superficie constante con cwd y entorno hostiles | `handshake.rs::the_surface_is_constant_whatever_the_cwd_and_the_environment`; `server::tests::announces_only_the_tools_capability` |
| SEC-MCP-08: entrada basura sin eco | `handshake.rs::garbage_on_stdin_ends_with_a_fixed_code` |
| Contrato con la CLI real y S-MCP-3 con `--scope user` | `apps/cli/tests/mcp_real_claude.rs` (`#[ignore]`; se ejecuta a mano con `--ignored` en una máquina con Claude Code; verde en macOS con 2.1.284) |

## 5. Pendientes

- `raptor doctor`: detectar un `gitraptor` de proyecto (`.mcp.json`) que no apunta al binario instalado (SEC-MCP-10, S-07) y repetir la sonda de S-MCP-3 por versión de Claude Code (ADR-MCP-001, Evidencia, punto 4). Dueño: la historia que cree `raptor doctor`.
- `cargo-deny` que prohíba las features de red de `rmcp` (SEC-07, SEC-MCP-09): US-MCP-003, junto con la comprobación estática de la frontera de `apps/mcp`.
- Conexión perezosa al canal con perfil `mcp` y su prueba de arranque bajo demanda: US-MCP-003.
- Linux y Windows: CLI `claude` (en Windows puede ser `claude.cmd`, que pasa por `cmd.exe`), permisos por ACL y carpetas temporales. **Pendiente: etapa de validación multiplataforma** (S-MCP-4, DEP-MCP-9). Las pruebas de `mcp_install.rs` son solo Unix.

## Estado de la implementación (2026-10-08)

Implementado en: PR #70.

Notas (fuera del alcance de esta ficha o sin bloquearla):
- La parte de `raptor doctor` (SEC-MCP-10) espera a la historia que cree `doctor`; `cargo-deny` sobre `rmcp` (SEC-07) sigue pendiente.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
