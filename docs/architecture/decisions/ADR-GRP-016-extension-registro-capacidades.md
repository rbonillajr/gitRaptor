---
id: ADR-GRP-016
title: "Extensión por registro y negociación de capacidades: sin archivos compartidos para una flota de agentes"
type: adr
status: accepted
accepted: 2026-10-06
date: 2026-10-06
created: 2026-10-06
updated: 2026-10-06
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-002, ADR-GRP-005, ADR-CKP-003, ADR-GRP-003, TS-GRP-004, API-GRP-IPC]
tags: [flota-de-agentes, conflictos, hotspots, registro, capacidades, protocolo, json-rpc, codigos-de-error, i18n, cli, daemon, monorepo]
---

# ADR-GRP-016 — Extensión por registro y negociación de capacidades

> **Estado**: aceptado (2026-10-06). **Decisión del orquestador (2026-10-06), validada por el Arquitecto.** Es un refactor sin cambio de comportamiento salvo el protocolo 9, que solo añade cosas. La guía práctica está en [extender-sin-archivos-compartidos.md](../extender-sin-archivos-compartidos.md).

## Contexto

GitRaptor se construye con una flota de agentes en paralelo: una rama, un worktree y un agente por historia (AGENTS.md). En dos días hubo **más de 30 re-subidas `-vN`** de PRs. También hubo choques semánticos que el rebase no ve: códigos JSON-RPC duplicados, campos nuevos en structs, variantes de enum y tablas TOML duplicadas.

Casi todas las historias tocaban los mismos seis archivos:

| Archivo | Qué editaba cada historia |
|---|---|
| `crates/api/src/lib.rs` | `PROTOCOL_VERSION` y `API_VERSION`, un contador global |
| `crates/api/src/methods.rs` | La lista central `METHODS` |
| `crates/api/src/rpc.rs` | El módulo `code` y el enum `ErrorCode` |
| `apps/cli/i18n/{en,es}.txt` | Un catálogo por idioma |
| `crates/core/src/daemon/mod.rs` | Un campo, su inicio y una llamada en cada punto del ciclo de vida |
| `apps/cli/src/main.rs` | La enum `Command` y su `match` |

Un archivo que edita cada historia **serializa la flota**: el segundo PR en llegar siempre tiene que rebasar. Y cuando dos historias eligen el siguiente número libre (un código, una versión), el rebase no avisa.

## Decisión

Cada cosa nueva se **declara en un archivo propio de su módulo** y se **registra por su presencia o con una línea por módulo**, nunca por historia. Un test de arquitectura falla si se vuelve a declarar algo en un archivo central.

### 1. Versionado por capacidades (enmienda a ADR-GRP-005 § 4 y § 5)

- **El protocolo 9 es el último para cambios aditivos.** `PROTOCOL_VERSION = 9` y `API_VERSION = "9.0.0"` quedan congelados. Solo se vuelve a subir si se **quita** algo, y eso exige un ADR.
- **Un método nuevo no necesita capacidad.** Se ofrece a toda conexión de protocolo 9 o superior, y el cliente lo descubre en `hello.methods` (como hasta ahora con `offers`). Los métodos de los protocolos 5 a 8 conservan su `since` congelado.
- **Una capacidad es un cambio de forma**: un tipo de evento nuevo, un campo nuevo o un código nuevo de un método que ya existe. Se nombra `<módulo>.<feature>` (por ejemplo `events.git-reset`), sin número de versión. Un número por grupo volvería a crear un contador compartido dentro del grupo, y además las features no tienen un orden lineal.
- **Las formas que trajo un número de protocolo pasan a ser capacidades legadas**, implícitas para ese protocolo y para el 9:

  | Capacidad | Protocolo | Qué cambia |
  |---|---|---|
  | `connection.requester` | 6 | `hello.requester` (ADR-CKP-003 § 4 N5) |
  | `events.git-reset` | 8 | Eventos Git de tipo `reset` (US-TMC-004) |

- **`hello` no cambia de forma.** Un daemon de protocolo 8 o anterior lee los parámetros antes que la versión, así que un campo nuevo lo cerraría con `INVALID_PARAMS` y rompería el reemplazo. Un cliente de protocolo 9 manda el `hello` de siempre:
  - Un daemon de protocolo 8 o anterior responde `-32002`, y se aplica el reemplazo que ya existe.
  - Un daemon de protocolo 9 añade a `HelloResult` el campo `capabilities`, con todas las que sirve. Solo lo envía a conexiones de protocolo 9, porque las de 5 a 8 rechazan campos desconocidos.
- **`connection.accept`** (protocolo 9, todos los perfiles, no reservado). Recibe `{capabilities: [nombre]}` y devuelve `{capabilities}` con las que tiene la conexión. Se llama una sola vez y antes de la primera suscripción, para que ningún evento ya recibido cambie de forma. Ignora los nombres que no sirve y acepta como mucho 64, de 64 caracteres cada uno. Una conexión que no lo llama recibe las formas del protocolo 8. El cliente de `crates/core` lo llama solo si conoce alguna capacidad posterior al 9 que el daemon sirva. **Hoy no hay ninguna**, así que no se envía.
- **Reemplazo con el mismo protocolo.** `ReplaceParams` no cambia. Desde el protocolo 9, un daemon acepta un `protocol` igual al suyo **solo** si viene del binario instalado y actualizado (`is_installed_replacement`, el criterio real de SEC-13). Si no, responde `-32602` y la conexión sigue. El cliente lo pide cuando el daemon no anuncia una capacidad que él conoce. Si el daemon lo rechaza, el cliente sigue con lo concedido, sin bucle. Con un daemon de protocolo 8 o anterior sigue valiendo solo un protocolo más nuevo.
- **Las comprobaciones de protocolo pasan a ser comprobaciones de capacidad.** `conn.rs` tenía tres: el `requester` de `hello`, el filtro de `reset` en la salida y el de `events.history`. La comprobación por método (`exists_in`) se queda igual para los protocolos 5 a 8.

### 2. Registro de métodos por módulo

- `crates/api/src/methods/` tiene **un archivo por módulo del contrato**: `connection`, `engine`, `events`, `sessions`, `audit`, `daemon`, `repo`, `attribution`, `registration`, `operation`, `requester`, `timemachine`, `scope` y `guard`. Cada archivo declara sus constantes de nombre y un `Group` con sus métodos, notificaciones, capacidades y errores.
- `methods/mod.rs` guarda los tipos y **tres líneas por módulo**: `mod x;`, `pub use x::*;` y su entrada en `GROUPS`, que fija el orden de `hello.methods`. Solo se toca para crear un módulo nuevo, algo raro. Las constantes de siempre (`methods::GUARD_PLAN`, …) siguen funcionando, y si dos módulos declaran el mismo nombre, la compilación falla.
- `METHODS` pasa a ser un `LazyLock<Vec<MethodSpec>>` sobre `GROUPS`, con la misma interfaz `.iter()`.
- **Tests:**
  - Cada nombre (método, notificación y capacidad) empieza por el prefijo de su módulo. `hello` y `ping` son de `connection`.
  - No hay nombres repetidos.
  - Un test de referencia fija, para los protocolos 5 a 8 y los perfiles completo y MCP, los métodos que se ofrecen y las formas implícitas (`crates/api/tests/legacy_protocols.rs`).

### 3. Códigos de error por bloques (enmienda a ADR-CKP-003 § 4 N7)

- **La lista compartida se congela en `-32016`.** El módulo `rpc::code` y el enum `ErrorCode` conservan sus 21 códigos y su número en el cable, y no se les añade ninguno. No se elimina `ErrorCode`: `apps/cli/src/codes.rs` necesita su `match` exhaustivo para cumplir V8.
- **Cada módulo tiene su bloque de 20 códigos**, que declara en su propio archivo con `error_block`. Los bloques empiezan en `-33000` y bajan de 20 en 20, fuera del rango que reserva JSON-RPC (`-32768..=-32000`). Cada error nuevo es un `ErrorSpec { code, name }` dentro de su bloque.
- **`rpc::error_name(code)`** busca en la lista congelada y en los módulos. El cliente presenta todos los errores igual, con la clave `error.<name>`.
- **Un código nuevo de un método que ya existe es un cambio de forma.** Necesita una capacidad, y quien no la tenga recibe un código de la lista congelada.
- **Tests:**
  - Cada código está dentro del bloque de su dueño.
  - Ningún bloque se solapa con otro, y todos empiezan en un múltiplo de 20 por debajo de `-33000`.
  - No hay códigos ni nombres repetidos, ni en los módulos ni en la lista congelada.
  - El test de arquitectura falla si `rpc.rs` declara un código nuevo.

### 4. i18n por feature (enmienda a ADR-CKP-003 § 10)

- Los catálogos de la CLI pasan a `apps/cli/i18n/<idioma>/<feature>.txt`, por ejemplo `en/guard.txt` y `es/guard.txt`. `apps/cli/build.rs` registra **cada archivo que encuentra**, en orden determinista, y Cargo vuelve a compilar si cambia el directorio. El formato `clave = texto` y el resultado en ejecución no cambian.
- **La garantía de ADR-CKP-003 § 10 ahora la dan tests, no el compilador.** En el catálogo de cadenas de la CLI ya era así. El catálogo tipado de la TUI (`present/i18n.rs`) sigue con su `match` exhaustivo. Los tests comprueban cuatro cosas:
  - Los dos idiomas tienen los mismos archivos.
  - Cada archivo tiene las mismas claves y los mismos marcadores en `en` y en `es`.
  - Ninguna clave aparece dos veces, ni dentro de un archivo ni entre archivos.
  - Cada **grupo** de claves (el texto antes del primer punto) vive en un solo archivo.
- **No se exige que el prefijo coincida con el nombre del archivo**, como proponía el Arquitecto. El archivo `contract` agrupa las claves que la CLI deriva de los códigos del contrato (`error.*`, `invalid.*`, `scope.*`, `layer.*`, …). Con la regla de un grupo por archivo, dos historias no editan el mismo archivo por el mismo grupo, que es la garantía que se buscaba. **Decisión del orquestador (2026-10-06)**: así se adaptó la recomendación del Arquitecto.
- Los inputs de Nx ya incluyen `apps/cli/i18n/**`, a través de `{projectRoot}/**/*`.

### 5. Daemon y CLI registrables

- **CLI:** cada subcomando de primer nivel vive en `apps/cli/src/commands/<nombre>.rs`, con sus argumentos (`Cmd`), sus acciones y su manejador. `commands/mod.rs` los registra con una macro, **una línea por subcomando**, en el orden de `--help`. `main.rs` solo parsea y despacha. Las utilidades compartidas pasan a `support.rs`. Para añadir una acción a un subcomando que ya existe (por ejemplo `guard uninstall`) solo se edita su archivo. La ayuda de todos los subcomandos queda **idéntica byte a byte**, comprobado contra la captura previa.
- **Daemon:**
  - Primero un commit que **solo mueve código**: `daemon/mod.rs` conserva la vida del daemon (inicio, bucle y parada). Los repos pasan a `repos.rs`, el canal a `serve.rs` y Guardrails a `guard.rs`.
  - Después, `daemon/modules/` con el trait `DaemonModule`, que tiene cuatro puntos con implementación por defecto: `repo_retired`, `observer_hooks`, `git_event` y `stop`. La lista `MODULES` va en orden y lleva una línea por módulo.
  - Un módulo arranca con los repos ya abiertos, justo antes de que empiece la observación. Para cuando el observador y el detector de sesiones ya pararon, y antes de que se cierren los almacenes. Recibe los handles tipados que necesita (`Arc<RepoMarks>`), así que no hace falta ningún downcast.
  - **La captura continua de la Time Machine es el primer módulo**, y sus llamadas conservan el sitio y el orden. Un test fija ese orden en cada punto y la lista de módulos registrados.
  - **La migración es parcial, de forma deliberada.** El observador, el detector de sesiones, los almacenes, los recursos y Guardrails siguen siendo partes del motor. Pasa a módulo cada feature cuando la toca su historia.

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| `linkme` o `inventory` (*distributed slices*) | Añaden una dependencia, y el enlazador puede descartar los elementos de una rlib a la que nadie referencia. En un registro que decide qué métodos existen, ese fallo sería silencioso |
| `build.rs` que escanee los módulos de código | Es magia de compilación y empeora el IDE. Solo se usa para datos (i18n), donde no declara módulos |
| Añadir `capabilities` a `hello` | Un daemon de protocolo 8 o anterior cerraría con `INVALID_PARAMS` antes de mirar la versión, y obligaría a un reintento frágil |
| Versión por grupo (`guard: 2`) | Vuelve a crear el contador compartido dentro del grupo y supone un orden lineal que las features no tienen |
| Seguir subiendo `PROTOCOL_VERSION` | Es el punto de conflicto que motiva este ADR |
| Renumerar los códigos legados por bloques | Rompe el cable con los clientes que ya existen |

## Consecuencias

- **Positivas**:
  - Una historia que añade un método, un error, un mensaje o una acción de un subcomando solo toca archivos de su módulo. Si dos historias eligen el mismo código o el mismo nombre, CI falla, porque los PRs se rebasan y se prueban al día.
  - El protocolo deja de ser un contador.
- **Negativas**:
  - Crear un **módulo** nuevo (del contrato, del daemon o un subcomando de primer nivel) sigue tocando una línea compartida. Es raro, y el conflicto se resuelve en una línea.
  - El dispatch de `crates/core/src/channel/conn.rs` (el `match` de métodos) sigue siendo central. Queda como pendiente.
  - Un reemplazo con el mismo protocolo se registra en el perfil como `replace:9`. El daemon nuevo lo clasifica como caída, no como parada atribuida, porque `replaced_by_newer` exige un protocolo menor. Es conservador, y se afinará cuando exista la primera capacidad posterior al 9.
- **Riesgos**:
  - **Un cliente que «conoce» una capacidad debe entender su forma.** Regla: la historia que declara una capacidad adapta en el mismo PR a todos los clientes del repo (CLI, TUI y `raptor-mcp`).

## Validación

- `cargo clippy --all-targets -- -D warnings` y `cargo test` en verde en macOS. El CI cubre Linux. **Windows y Linux no se verificaron desde este Mac**: lo hace el CI.
- `crates/api/tests/architecture.rs`:
  - `methods/mod.rs` no declara nombres, capacidades ni errores.
  - `rpc.rs` conserva los 21 códigos congelados y ninguno más.
  - El protocolo sigue congelado en 9.
  - Ningún método espera a un protocolo más nuevo.
- `crates/api/tests/legacy_protocols.rs`: lo que ven los clientes de los protocolos 5 a 8.
- `crates/core/tests/channel_capabilities.rs`:
  - `hello` solo anuncia las capacidades a las conexiones de protocolo 9.
  - `connection.accept` se acepta una sola vez y antes de suscribirse.
  - El reemplazo con el mismo protocolo solo lo acepta el binario instalado, solo desde el protocolo 9.
  - Si el daemon rechaza el reemplazo, el cliente sigue sin bucle.
- `--help` de cada subcomando idéntico al anterior. Los snapshots de la TUI no cambian.

## Referencias

- [ADR-GRP-005](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md) § 4 y § 5 (enmienda del 2026-10-06), [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md) § 4 N7 y § 10 (enmienda del 2026-10-06).
- [Contrato del canal](../design/api-contract-ipc.md) (handshake, métodos y errores).
- [Guía: cómo extender sin tocar archivos compartidos](../extender-sin-archivos-compartidos.md).
