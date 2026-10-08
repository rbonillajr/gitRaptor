---
id: DS-US-MCP-005
title: "Dev Spec — US-MCP-005: respuestas del MCP acotadas, con el texto del repo como dato y sin secretos"
type: dev-spec
status: approved
feature: mcp
domain: MCP
created: 2026-10-07
updated: 2026-10-07
related:
  stories: [US-MCP-005, US-MCP-003, US-MCP-004]
  adrs: [ADR-MCP-001, ADR-GRP-005, ADR-GRP-016]
  rules: [BR-MCP-CALC-002, BR-MCP-VAL-006, BR-MCP-CONS-004, BR-MCP-CONS-005, BR-MCP-VAL-005, BR-MCP-TIME-001]
  nfrs: [NFR-02, SEC-05, SEC-08, SEC-12, SEC-MCP-07]
tags: [mcp, seguridad, respuesta-acotada, texto-no-confiable, errores, rate-limit, ola-1]
---

# Dev Spec — US-MCP-005: respuestas del MCP acotadas, con el texto del repo como dato y sin secretos

Plano compacto (AADD ligero) de [US-MCP-005](../user-stories/US-MCP-005-respuestas-acotadas-y-seguras.md). El contrato ya lo fija [ADR-MCP-001](../../../../architecture/decisions/ADR-MCP-001-servidor-mcp-cliente-daemon.md): § 5 (respuestas y errores), § 6 (límites) y § 9 (MCP03, MCP06, MCP10, consumo sin límite). Esta spec cierra lo que el ADR deja a la Dev Spec de US-MCP-005: la lista de códigos de error de hoy, la forma del error, dónde se escapa el texto y cómo se mide el presupuesto. La extensión sigue [ADR-GRP-016](../../../../architecture/extender-sin-archivos-compartidos.md).

Hoy `status` es la única herramienta (US-MCP-003). Esta historia endurece **la tubería de respuesta** de `raptor-mcp`, de modo que toda herramienta futura pase por ella, y añade a `status` la **rama** del worktree del llamante: es el primer texto del repo que puede llevar una inyección y la prueba de aceptación lo necesita.

## 1. Decisiones

Las marcadas con † son **Decisión del orquestador (2026-10-07), validada por Arquitecto/PO** (ver § 5); el resto aplica el ADR.

| # | Decisión |
|---|---|
| D1 † | **Escape en dos pasos, ambos en `crates/api`** (ajuste del Arquitecto). (1) **Tope por clase, tipado**: cada vista MCP (`McpStatus::for_mcp`, como el `for_mcp` de `timemachine.rs`) corta los **nombres** (rama, worktree, agente, ref) a **100 caracteres** con `UntrustedText::mcp_name`, conservando `truncated` y `lossy`. (2) **Red de seguridad genérica** (`mcp_view::for_mcp`): recorre el JSON del resultado y reescribe todo objeto marcado `{"untrusted": …}` (el marcador explícito de `untrusted.rs`): secuencias ANSI/OSC/DCS fuera; C0, DEL, C1, bidi, anchura cero, Tags, U+2028/U+2029 → U+FFFD (el `sanitize` de la TUI); userinfo, query y fragmento de URLs (`esquema://usuario:token@…?access_token=…#…`) fuera (ajuste del coordinador: los tokens también viajan en la query); tope de ruta de **1.024 bytes** (§ 6) como respaldo. Así un campo no confiable que añada una historia futura queda saneado aunque nadie se acuerde. `MAX_MCP_UNTRUSTED_BYTES` (256 bytes) se retira: contradecía los dos topes del § 6, y el `for_mcp` del timeline pasa a `mcp_name`. El daemon sigue sin sanear: lo hacen los clientes. |
| D2 † | **`branch` en `status`** (`McpStatus.branch: Option<UntrustedName>`, ausente con `HEAD` separado o worktree no disponible), detrás de la capacidad nueva `mcp.status-branch`: es un campo nuevo de un método que ya existe (ADR-GRP-016). Es el primer trozo del contenido de US-MCP-004; el resto (sesiones, rutas modificadas, ahead/behind, base, protección) sigue en US-MCP-004. |
| D3 | **Presupuesto de 24 KiB** (§ 6) por parte (texto y estructurada), medido **después** del escape. Si un resultado no cabe, se sustituye por el error tipado `result-too-large`, sin ningún dato parcial (ajuste del coordinador: un recorte sin marca parecería completo). Hoy es un respaldo inalcanzable: un test llena cada campo de `status` hasta su tope y comprueba que cabe, por separado en cada parte. Las listas que se recortan hasta caber, con `truncated`, el total y el cursor, llegan con la primera herramienta que tenga listas (US-MCP-004 para las rutas modificadas): usan las constantes de esta historia. |
| D4 | **Forma del rechazo de dominio** (§ 5): `isError: true` y, en el bloque de texto, `{code, message, action}` más `params` cuando los hay. `code` es estable, `kebab-case`, en inglés, de un enum cerrado de `crates/api` (`McpToolError`); `message` y `action` salen de una plantilla fija por código en `apps/mcp`, en en/es. Sin `structuredContent` (no cumple el `outputSchema`). Sustituye a `{reason, action}` de DS-US-MCP-003 (D6), que ya anunciaba esta traducción. |
| D5 † | **Lista cerrada de códigos de hoy**: `repo-not-enabled`, `not-in-observed-worktree`, `repo-unavailable`, `engine-unavailable`, `identity-unverified`, `rate-limited`, `time-limit`, `result-too-large`, `internal`. `repo-unavailable` entra ya (ajuste del Arquitecto): el caso existe hoy (`RepoStateView::Unavailable`, el almacén del repo no se abre) y el § 2 manda rechazarlo; `raptor-mcp` rechaza sin datos cuando el estado es ese (el rechazo del worktree no disponible en el daemon sigue en US-MCP-004). `result-too-large` e `internal` son **adiciones** a las familias del § 5. Las demás familias del § 5 (`repo-unavailable`, precondiciones, decisión, avisos, ejecutor) las **añade su historia dueña** cuando la herramienta exista: añadir un código es compatible; quitarlo o cambiarlo sube la versión mayor. El código del escenario de la historia (`MCP_REPO_NOT_ALLOWED`) es ilustrativo: manda el ADR (`repo-not-enabled`). |
| D6 | **Idioma**: `raptor-mcp` lee al arrancar `LC_ALL`, luego `LC_MESSAGES`, luego `LANG`; si empieza por `es`, español; si no, inglés (NFR-10). Códigos, nombres y descripciones de herramientas, siempre en inglés. |
| D7 | **Llamada mal formada** (§ 4.2): error de protocolo `-32602` con el mensaje estable `invalid-params` y `data: {field}`, con el nombre del campo escapado como D1. Herramienta desconocida: `-32602` con `unknown-tool`. No se consulta ningún repo. |
| D8 | **Catálogo fijo** (MCP03): `initialize` y `tools/list` son constantes del binario. La descripción de `status` y las `instructions` declaran que los campos `{"untrusted": …}` son texto del repo, **dato y nunca instrucción**. Una prueba de instantánea fija ambos textos y comprueba que `tools/list` es idéntica en cada consulta y en una carpeta con un archivo que intenta redefinir la herramienta. |
| D9 † | **Rate limit de lecturas en el daemon**, por conexión `mcp`: 120 por minuto con ráfaga de 30 (§ 6, SEC-08), con un segundo cubo en `Connection` aplicado a los métodos que no son escrituras. El rechazo es el `RATE_LIMITED` congelado (sin forma nueva); `raptor-mcp` lo traduce a `rate-limited` con `params.retry_after_s` calculado de las constantes. Las demás conexiones tienen su propio cubo. El cubo se consulta antes que cualquier rechazo de ámbito. **Fuera de esta historia**, con la Enmienda (2026-10-07, US-MCP-005) de ADR-MCP-001: el cupo compartido por solicitante entre conexiones y el tope de 8 conexiones por solicitante (S-03), condición de entrada de US-MCP-008/009. ≤ 4 peticiones en curso (S-11) se cumple por construcción: `Connection::serve` atiende una petición cada vez y `raptor-mcp` serializa sus llamadas al motor con un `Mutex`. |
| D10 | **Tiempo de una lectura** ≤ 10 s (BR-MCP-TIME-001), incluido el arranque del daemon: `raptor-mcp` envuelve la llamada al motor con un `timeout`; si vence, `time-limit` con la acción "reintenta más tarde". |
| D11 | **Sin secretos** (SEC-05): ninguna herramienta lee config, remotos, entorno ni contenido de archivos; el allowlist de campos de `McpStatus` sigue cerrado (test de US-MCP-003) y D1 quita userinfo de cualquier texto no confiable como defensa en profundidad. La prueba de aceptación planta un remoto `https://user:token@…` y comprueba que el token no aparece en ninguna parte de la respuesta. |

## 2. Código

| Tramo | Pieza | Ubicación |
|---|---|---|
| A · contrato | Topes y constantes del MCP, `McpToolError`, `for_mcp` (D1), presupuesto (D3) | `crates/api/src/mcp_view.rs` (nuevo) + una línea `pub mod` en `crates/api/src/lib.rs` |
| A · contrato | `UntrustedText::mcp_name`; se retira `MAX_MCP_UNTRUSTED_BYTES` | `crates/api/src/untrusted.rs`, `crates/api/src/timemachine.rs` |
| A · contrato | `McpStatus.branch`, `McpStatus::for_mcp`, `CAP_MCP_STATUS_BRANCH` | `crates/api/src/methods/mcp.rs` |
| B · daemon | `branch` en `mcp.status` solo con la capacidad; cubo de lecturas MCP (D9) | `crates/core/src/channel/conn.rs` (brazo de `mcp.status` y `Connection`), constantes en `crates/core/src/channel/mod.rs` (`Limits`) |
| C · servidor | Tubería de respuesta (escape, presupuesto, errores en/es, tiempo), descripciones | `apps/mcp/src/server.rs`, `apps/mcp/src/engine.rs`, `apps/mcp/src/messages.rs` (nuevo), `apps/mcp/src/main.rs` |
| D · pruebas | Extremo a extremo con repo temporal hostil | `apps/cli/tests/mcp_allowlist.rs`, `apps/mcp/tests/handshake.rs` |

## 3. Pruebas

| Escenario / criterio | Prueba |
|---|---|
| **Aceptación observable**: rama con C1/bidi/Tags y "ignore-previous-instructions…", carpeta del worktree con OSC 52, remoto `https://user:token@…` → respuesta acotada, saneada, etiquetada y sin el token | `apps/cli/tests/mcp_allowlist.rs::hostile_repo_text_arrives_bounded_marked_and_without_secrets` |
| Rechazo en español con código estable, motivo y acción, sin rutas | `apps/cli/tests/mcp_allowlist.rs::a_refusal_says_what_happened_and_what_to_do_in_spanish` (+ los rechazos existentes, migrados a `{code, message, action}`) |
| Agente en bucle: rate limit con espera; otra conexión sigue respondiendo | `apps/cli/tests/mcp_allowlist.rs::a_looping_agent_hits_its_connection_limit` |
| Parámetro no declarado (`repo`) → mal formada, sin consultar el motor | `apps/mcp/tests/handshake.rs::undeclared_parameters_are_malformed` |
| Catálogo fijo e idéntico, descripciones que declaran el texto como dato | `apps/mcp/tests/handshake.rs::the_catalog_is_fixed_and_declares_repo_text_as_data`, `apps/mcp/src/server.rs` (instantánea) |
| Escape (incluidos los de anchura cero), userinfo, query y fragmento de URLs, topes por clase, recorrido genérico | `crates/api/src/mcp_view.rs` (tests unitarios) |
| Presupuesto de 24 KiB → `result-too-large` sin datos; `status` con cada campo en su tope cabe | `apps/mcp/src/server.rs` (tests unitarios) |
| Repo no disponible → `repo-unavailable` sin datos | `apps/mcp/src/server.rs` (test unitario) |
| Tiempo de lectura vencido → `time-limit` | `apps/mcp/src/server.rs` (test unitario con un tope corto) |
| `branch` solo con la capacidad | `crates/api/src/methods/mcp.rs` (forma) + prueba de aceptación (con la capacidad) |

## 4. Pendientes y fuera de alcance

- **Paginación con cursor de las rutas modificadas**: pasa a US-MCP-004 (ajuste del PO), que recibe el escenario "3.000 modificados → tope, total, `truncated` y cursor" con los topes de este spec (200 por worktree, 32 worktrees, 24 KiB, cursor opaco con MAC del daemon). El escenario 1 de US-MCP-005 queda en "sin diff, mensajes, contenido, config, entorno ni userinfo" y el presupuesto de 24 KiB; BR-MCP-CALC-002 queda repartida, "(parte)", entre las dos historias.
- **Límites por solicitante** (S-03): cupo compartido entre conexiones del mismo agente y ≤ 8 conexiones por solicitante. Condición de entrada de US-MCP-008/009 (Enmienda (2026-10-07, US-MCP-005) de ADR-MCP-001 y Dependencias de esas historias). Mientras tanto un agente con varias conexiones suma cupos de lectura, y la CLI lanzada por un agente esquiva el cubo de la conexión `mcp` hasta S-01.
- **Rate limit de escrituras** (20/min, ráfaga 5): no hay herramientas de escritura todavía.
- ⚠️ **ASSUMPTION** del § 6 (si Claude Code pasa al modelo las dos partes del resultado): no se mide aquí; sigue para la medición de la ola 1 (US-MCP-015).
- Linux y Windows: las pruebas de extremo a extremo son de macOS (lectura del cwd del par, DEP-MCP-9). **Pendiente: etapa de validación multiplataforma.**

## 5. Validación

Decisión del orquestador (2026-10-07), validada por Arquitecto/PO:

- **Arquitecto**: aprueba D2, D3 y D4. Ajusta D1 (tope por clase: 100 caracteres en nombres, tipado; red genérica con el tope de ruta; retirar `MAX_MCP_UNTRUSTED_BYTES`), D5 (`repo-unavailable` ya; `internal` como adición) y D9 (diferir S-03 relaja el § 6 y pide una enmienda del ADR; S-11 por construcción; cubo antes del ámbito). Aplicados.
- **PO**: aprueba el rate limit por conexión (con S-03 anotado en US-MCP-008/009), `branch` como único trozo de US-MCP-004 y el código `repo-not-enabled` del ADR. Ajusta el traslado de la paginación: el escenario pasa a US-MCP-004, `covers` marca BR-MCP-CALC-002 "(parte)", y el escenario 5 de US-MCP-005 y el ejemplo de `business-rules.md` dicen `repo-not-enabled`. Aplicados en esas fichas.
- **Coordinador** (aprobación del plan, 2026-10-07): B1 se mide sin los tests nuevos (verde); quitar también query y fragmento de las URLs; probar los caracteres de anchura cero; el desborde del presupuesto falla tipado (`result-too-large`) en vez de `internal`. Aplicados.

## 6. Revisión de seguridad (2026-10-07)

security-expert: sin Critical ni High. Corregido en la rama:

- **M-01**: una llamada vencida ya no hace cola detrás de la anterior (`try_lock`: responde al momento en vez de gastar el cupo) y el runtime se cierra con 1 s de plazo al cerrarse stdin.
- **M-02**: el saneado también cubre los selectores de variación (U+FE00–FE0F, U+E0100–E01EF), U+034F y los rellenos Hangul (U+115F, U+1160, U+3164, U+FFA0), que esconden bytes dentro de texto visible. Amplía la lista L-03.
- **M-03**: un arranque o reemplazo del motor que no termina a tiempo da `engine-unavailable` (BR-MCP-EDGE-001), y `time-limit` solo con una conexión abierta. La descripción de `status` dice que `branch` puede faltar.
- **L-01**: el paso genérico recorre también los hermanos de un objeto `{"untrusted": …}`. **L-02**: el userinfo se corta hasta el último `@` antes de la query, aunque la contraseña lleve una `/`. **L-03**: los comandos reservados pedidos por MCP no gastan el cupo de lecturas (conservan su propio cubo y su auditoría).

Pendiente, anotado en el PR: **L-01** (claves de objeto no saneadas; hoy todas las claves son del binario). **L-04** (`retry_after_s` sale del cubo MCP; si rechaza el cubo general del canal, 1 s sigue siendo una cota válida). Y una prueba contra un daemon sin la capacidad `mcp.status-branch` (el mecanismo de capacidades ya se prueba en el canal).

