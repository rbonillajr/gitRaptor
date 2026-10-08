---
id: DS-INF-CKP-001
title: "Dev Spec — Esqueleto de la TUI: bucle TEA, cliente del canal, saneado único y microbanco de 100 ms"
type: dev-spec
status: approved
feature: cockpit
domain: GRP
story: INF-CKP-001
created: 2026-10-05
updated: 2026-10-08
related:
  adrs: [ADR-CKP-003, ADR-GRP-011, ADR-GRP-005, ADR-GRP-004, ADR-GRP-002]
  nfrs: [NFR-04, NFR-10, SEC-01, SEC-08, SEC-12]
  stories: [INF-CKP-001, TS-GRP-004, TS-CKP-004, TS-CKP-005, US-CKP-001, INF-GRP-002]
tags: [cockpit, tui, ratatui, tea, canal, resync, reconexion, saneado, sec-12, latencia, nfr-04, i18n, fronteras, ci]
---

# Dev Spec — INF-CKP-001: esqueleto de la TUI

Blueprint compacto de [INF-CKP-001](../technical-stories/INF-CKP-001-esqueleto-tui.md). Sigue ADR-CKP-003 § 1 a § 8, § 10 y § 12. La INF se entrega en **dos partes**. Esta Dev Spec fija la **Entrega 1**, implementada en la rama `feat/INF-CKP-001-tui-skeleton`, y lista lo que queda para la **Entrega 2**.

> **Decisión del orquestador (2026-10-05), validada por Arquitecto**: partición en dos entregas, frontera con un trait de transporte, estampado de `t_client_recv` al leer el frame y microbanco sintético en `cargo test`. El Arquitecto pidió dos ajustes, ya aplicados: adelantar `SafeText` y el saneado a esta entrega, y usar el hook de pánico de `ratatui::init()` en lugar de uno propio. Detalle en § 8.

## 1. Ubicación en el código

`apps/cli` gana un **target de biblioteca interno** (`gitraptor_cli`, `src/lib.rs`, no publicado) para que las pruebas y el banco conduzcan la `App` sin pantalla (§ 12). No hay crates nuevos.

| Módulo | Responsabilidad |
|---|---|
| `model` | `Model` (réplica del motor, `ui`, `conn`, `now_ms`, `dirty`), `Msg`, `Cmd`, `ConnState`, `Stamped<T>` |
| `client` | Traits `Connector` y `Link`. Hilo del canal: conexión, instantánea por ámbito y suscripción desde `N+1`, `repo.locate` del cwd, lectura del stream, comandos `Resync` y `Reconnect`, y espera de 250 ms a 5 s sin límite de intentos |
| `client::sequence` | `SeqTrack` puro: duplicado, siguiente, hueco y espera de instantánea |
| `present` | `SafeText` y saneado (§ 8), `ingest` (contrato → modelo) y el catálogo tipado `i18n` en/es |
| `queue` | Cola de entrada y cola del motor, más una cola de despertar porque `mpsc` no espera en dos receptores |
| `tui::app` | Bucle de un hilo, genérico sobre el backend. Por iteración: la entrada se drena entera, el motor hasta agotar el presupuesto de 8 ms, se hace un solo `draw` y se estampa `t_render` |
| `tui::update` / `tui::view` | Puros. `view` compone tres regiones con **marcadores**: cabecera, panel vacío y barra de estado. Por debajo de 80×24 solo pinta el mensaje de tamaño mínimo |
| `tui::keymap` | Tabla única acción ↔ teclas: `q`/`Ctrl-C` salir y `r` reintentar. Alimenta la entrada y las pistas |
| `tui::input` | Hilo de entrada con crossterm. Teclas, pegado y resize; en Unix, crossterm convierte SIGWINCH en resize |
| `tui::term` | `ratatui::try_init()`: modo raw, pantalla alternativa y hook de pánico que restaura. Un guard restaura al salir o ante un error. El primer frame se pinta antes de arrancar el hilo del canal |
| `tui::metrics` | Histograma local en memoria por etapa (decodificar, aplicar, pintar, total) y por tecla. Nunca sale de la máquina |
| `client::engine` | Implementa `Connector`/`Link` con la biblioteca cliente de `crates/api` (comprobación L-06 del par incluida); el arranque bajo demanda llega inyectado como `Launch`. Sustituye a la antigua excepción `link` (Entrega 2b, § 11) |

En el binario, `raptor` sin subcomando y `raptor tui` abren la TUI si stdin y stdout son TTY. Si no, salen con código 2 y sugieren `raptor status`.

## 2. Dependencias nuevas

| Dependencia | Dónde | Por qué | Licencia |
|---|---|---|---|
| `ratatui` 0.30.2 (`default-features = false`, `crossterm_0_29`, `layout-cache`) | workspace → `apps/cli` | ADR-CKP-003 § 1. crossterm llega reexportado (`ratatui::crossterm`): sin dependencia directa | MIT |

Sin `insta` ni `proptest`: los snapshots son aserciones sobre el `Buffer` de `TestBackend`, y el fuzzing es un generador xorshift con semilla fija. Si TS-CKP-005 incorpora `insta`, las vistas migran a él.

## 3. Cambio en `crates/core` (aditivo)

`Client::next_incoming(timeout) -> Option<Incoming>` devuelve el **frame crudo** con `monotonic_ns()` tomado al terminar de leerlo y **antes de decodificarlo** (§ 6). Las notificaciones que llegan mientras un `call` espera su respuesta se guardan con su marca de lectura (`VecDeque<(u64, Notification)>`): no se estampan al sacarlas, porque eso sesgaría el gate a favor de pasar. `next_notification` y `read_message` mantienen su API.

## 4. Flujo del cliente

1. `Connecting` (o `Reconnecting{intento}`) y luego `connect()`. Si falla: `EngineUnavailable`, `Rejected` o `Incompatible`, se espera `backoff(intento)` y se reintenta. `r` reintenta ya. `Unsupported` (Windows hoy) no reintenta.
2. Requester del handshake (N5), saneado. Después `Syncing`, `scope.snapshot` global y `scope.subscribe` desde `N+1` con su `run_id`. Si el daemon localiza el cwd (N4), lo mismo para ese repo. Luego `Live`.
3. Bucle: comandos pendientes, después `next(20 ms)`, `decode` y cola del motor.
4. En `update`: un duplicado se descarta. Un hueco marca el ámbito como desactualizado, pasa a `Resyncing` y pide `Cmd::Resync{resubscribe: false}`; la suscripción sigue viva y lo que llegue hasta la nueva instantánea se descarta. Un `scope.resync` hace lo mismo con `resubscribe: true`. `ScopeClosed` suelta el repo. Al reconectar, todo queda desactualizado (los datos se conservan) hasta la instantánea nueva.

## 5. Saneado (SEC-12)

`SafeText` solo se construye saneando: `text` (tope de 1.024 caracteres) y `name` (100 caracteres, L-03), o desde `UntrustedText`. C0, DEL y C1 se escriben como escapes visibles (`\x1b`, `\u{9b}`). También se escapan los controles bidi (incluido U+061C), los de anchura cero (U+200B–U+200F, U+2060–U+2064, U+FEFF), U+2028, U+2029 y los Tags. Se conservan como mucho dos marcas combinantes por carácter base, y un texto recortado termina en `…`. `present::ingest` es el único punto de entrada: el modelo no guarda ningún `Untrusted`. El recorte por anchura con elipsis lo hacen los widgets (TS-CKP-005).

## 6. Convención de widget (para TS-CKP-005)

Los widgets son puros, viven en `apps/cli/src/tui/widgets/<nombre>.rs` y cada uno es un struct que implementa `ratatui::widgets::Widget`:

- se construye con datos ya derivados: `SafeText`, texto de `present::i18n` o números;
- recibe los estilos del tema mapeados a `ratatui::style`;
- no lee `Model` ni el reloj.

`tui::view` solo reparte regiones y compone. Hoy pinta marcadores sin color, solo con atributos. US-CKP-001 los sustituye por los widgets.

## 7. Verificación (Entrega 1)

| Criterio | Prueba |
|---|---|
| Bucle TEA y secuencia | `tui::update` (duplicado, hueco sin aplicar nada después, `resync`, `ScopeClosed`, reconexión, otro repo ignorado, ninguna tecla cambia la réplica); `client::sequence` (propiedad: lo aplicado es contiguo y único) |
| Resync y reconexión con el hilo real | `tests/tui_loop.rs`: hueco → nueva instantánea sin resuscribir; `scope.resync` → instantánea y suscripción; canal perdido → reconexión con instantáneas nuevas |
| Gate de 100 ms (microbanco) | `tests/tui_loop.rs`: ráfaga de 1.000 `worktree.state` de 10 worktrees por el hilo del canal, con la decodificación incluida. Falla si el p95 de `t_client_recv` → `t_render` supera 100 ms y nombra la etapa más lenta. Falla si los frames llegan a 500. Si el feedback de tecla supera 100 ms, solo avisa. En macOS debug: p95 ≈ 19 ms, 44 frames |
| Coalescencia y entrada primero | `tests/tui_loop.rs`, determinista: con 1.000 mensajes encolados y una tecla detrás, la tecla se pinta en el primer frame, y los frames son ≤ 100 |
| Terminal | `tests/tui_process.rs` (macOS, `script`): `q` sale y deja la terminal restaurada; un pánico provocado en la vista (`GITRAPTOR_TUI_PANIC_IN_VIEW`, solo en debug) sale de la pantalla alternativa antes del mensaje del pánico. Resize: `update` + `view` a 79×24 |
| Sin TTY | `raptor` y `raptor tui` sin terminal salen con 2, sugieren `raptor status` y no arrancan el daemon |
| Varias TUIs | `tests/tui_process.rs` (macOS): dos `App` sin pantalla sobre el daemon real del perfil temporal llegan a "En vivo" con la misma réplica global |
| Fronteras (V5) | `tests/tui_boundaries.rs`: `tui`, `model`, `client`, `present` y `queue` no importan `gitraptor_core`, `gitraptor_git` ni `gitraptor_policy`; todo módulo de la lib está cubierto y `link` es la única excepción; `apps/cli` no depende de Git ni de políticas fuera de dev; la TUI no lanza procesos; el modelo y la vista no tocan `Untrusted` |
| Saneado (V4) | `present::sanitize`: corpus (OSC 52, título, `ESC[2J`, U+009B, RLO, U+061C, U+2028, U+2063, U+E0041), fuzzing de 20.000 entradas, nombre de 300 → 100 + `…`; vista con rama maliciosa sin ESC en el buffer |
| i18n | Catálogo tipado: un `match` exhaustivo por idioma; test de que cada `ConnState` tiene texto en y es |

## 8. Decisiones (orquestador, 2026-10-05, validadas por Arquitecto)

1. **Partición.** La Entrega 1 incluye bucle, cliente, `SafeText` y saneado, la ingesta mínima, el catálogo tipado mínimo, keymap, terminal, métricas, microbanco y fronteras. Ajuste del Arquitecto: `SafeText` entra ya para que TS-CKP-005 no invente un sustituto y el modelo no guarde texto en bruto.
2. **Frontera por trait.** `client::Link`/`Connector` y el módulo `link` como excepción con nombre hasta extraer el cliente a `crates/api` (hecho en la Entrega 2b, § 11: `link` ya no existe).
3. **`t_client_recv`** se estampa al leer el frame, también para las notificaciones guardadas durante un `call`. Ajuste del Arquitecto: estamparlas al sacarlas subestimaba la latencia.
4. **Microbanco en `cargo test`**, que corre en el check obligatorio "lint and test". No es el gate E2 con el daemon real. Lo deterministas, frames y entrada primero, es lo que falla de forma estable; el umbral de pared de 100 ms también falla, pero informa de la etapa. "Aplicar" es trivial hoy: **US-CKP-001 recalibra** el microbanco cuando llegue la flota.
5. **Pánico.** Se usa el hook de `ratatui::init()` y no uno propio, para no duplicar la restauración.
6. **`raptor tui`** es un alias explícito de `raptor` sin subcomando. Se anota en ADR-CKP-003 § 11.

## 9. Pendiente: Entrega 2 de INF-CKP-001

La Entrega 2 se parte en dos. La **2a** (UX) y la **2b** están hechas: ver las enmiendas de § 10 y § 11. Lo que pedía la 2b:

- Gate con el daemon real en el banco de INF-GRP-002 (E2 de ADR-GRP-011): la `App` sobre `TestBackend` como suscriptor del banco.
- Extraer el cliente del canal de `crates/core` a `crates/api` y retirar la excepción `link` (§ 5, V5).
- Pruebas L-06 desde la TUI (socket 0755, de otro uid o servidor falso: "Canal rechazado" sin handshake) y la suite "TUI sin perfil" de INF-GRP-001.
- Linux y Windows: **Pendiente: etapa de validación multiplataforma**. En Windows el canal no existe todavía (`Unsupported`), `Ctrl-Z` solo avisa y las pruebas de pty son solo de macOS.

## 10. Enmienda (2026-10-07): Entrega 2a, UX

Implementada en la rama `feat/INF-CKP-001-delivery-2-ux`. Sigue ADR-CKP-003 § 4, § 9 y § 10 sin decisiones de arquitectura nuevas.

| Punto | Qué se hizo | Prueba |
|---|---|---|
| `--lang` | Opción global `--lang en\|es` (también tras el subcomando) y variable `GITRAPTOR_LANG`. Orden: `--lang`, `GITRAPTOR_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG` y `en`. `Lang::choose` la fija una vez en `main`, y la usan el catálogo tipado de la TUI y el de la CLI (`i18n.rs`), así que es global de verdad. Un `GITRAPTOR_LANG` desconocido se ignora y decide el locale; un `--lang` desconocido lo rechaza `clap` | `present::i18n::tests::lang_precedence`; `tests/tui_process.rs::lang_flag_beats_the_locale` |
| Catálogo N7 (V8) | `Text::EngineError(ErrorCode)` con `match` exhaustivo de los 21 códigos congelados; `Text::ModuleError(name)` para los códigos de módulo (`error.<name>`, tabla `MODULE_ERRORS`, hoy vacía porque ningún módulo declaró uno); `Text::UnknownError(code)`; y los motivos tipados `ScopeRefusal`, `InvalidReason` y `ResyncReason`. `Text::error(code)` elige la variante | `present::i18n::tests::every_contract_code_has_both_languages`: texto en y es distinto para todos, y ningún código de módulo cae en el genérico |
| `Ctrl-Z` | Acción `Suspend` en el keymap (sin pista: es la convención de la terminal). `update` devuelve `Cmd::Suspend` en Unix y `Notice::SuspendUnsupported` en el resto. El bucle la ejecuta tras la entrada de su iteración: pausa el hilo de entrada **y espera su confirmación** (`input::Pause`, acotada a 500 ms; un hilo que ya terminó cuenta como pausado y, si uno vivo no confirma, no se entrega la terminal), restaura la terminal y muestra el cursor, envía `SIGTSTP` a su grupo de procesos como lo haría la terminal (`rustix`, feature `process`; `Cargo.lock` sin cambios), y al volver reentra en modo raw y en la pantalla alternativa, reanuda la entrada y fuerza un redibujado completo con `Terminal::resize` (no `clear`, que pregunta la posición del cursor), recogiendo el tamaño nuevo. `term::suspended(pause, f)` es el mecanismo que reutilizará el editor | `tui::update` (unitario); `tests/tui_loop.rs::ctrl_z_suspends_and_repaints`; `tests/tui_process.rs::pty::ctrl_z_restores_and_reenters` (macOS, sin `sleep`: espera la salida) |
| Arranque del motor | `ConnState::Starting` ("Arrancando el motor…"), distinto de `Connecting`. `crates/core` gana `ensure_daemon_with(options, on_launch)` (aditivo: `ensure_daemon` lo llama con un cierre vacío), que avisa justo antes de lanzar el daemon. `Connector::connect_starting` lo propaga al hilo del canal. `r` durante el arranque avisa "el motor está arrancando" y no encola una reconexión | `tests/tui_loop.rs::starting_the_engine_is_not_connecting` |
| Elipsis | El recorte por anchura con elipsis ya lo hacían `style::put` y `Pen` (TS-CKP-005) en todos los widgets: ninguno escribe sin pasar por ellos. Se añaden snapshots en y es con nombres largos y caracteres anchos, en Unicode (`…`) y ASCII (`...`) | `tui::view::tests::long_names_end_in_an_ellipsis`; snapshots `fleet_ellipsis_80x24_{en,es}` |

Fuera de la 2a, además de la 2b: el **lanzador del editor** (historia de Q-CKP-9, que reutiliza `term::suspended`) y **mostrar en la vista** un error RPC concreto, que hará la historia que lo provoque (el catálogo ya lo traduce). Con `script`, la TUI encabeza un grupo de procesos huérfano y el sistema descarta el `SIGTSTP`: la prueba de pty comprueba la salida y la vuelta de la terminal, no la parada en sí, que es control de trabajos del shell.

## 11. Enmienda (2026-10-08): Entrega 2b

Implementada en la rama `feat/INF-CKP-001-delivery-2b`. Cierra lo que § 9 dejaba para la 2b, salvo Linux y Windows reales.

> **Decisión del orquestador (2026-10-08), validada por Arquitecto con ajustes**: el cliente se extrae a `crates/api` **por inversión de dependencias**. `crates/api` define `trait Launch` y `crates/core` lo implementa. El autoarranque, el entorno limpio y el lock del daemon no pasan a api. Los ajustes del Arquitecto, ya aplicados: la feature `client` con las dependencias del SO opcionales; una sola implementación de lo que comparten cliente y servidor (ruta del socket, carpeta privada, nombre del pipe); `Connect.runtime` obligatorio; `Launch: Send`, con su error propio `ClientError::Launch`; y la enmienda fechada en ADR-CKP-003, porque el ADR decía que el lanzamiento vivía en api. El coordinador aprobó el plan y añadió tres notas: las comprobaciones L-06 existen una sola vez; `crates/api` mantiene `forbid(unsafe_code)` y la frontera de `unsafe` no cambia; y hay que probar en el Windows real.

| Punto | Qué se hizo | Prueba |
|---|---|---|
| (1) Gate E2 con el daemon real | Ya lo cubre el escenario `tui-modify` del banco INF-GRP-002, que entregó US-CKP-001 (D4): la `App` real sobre `TestBackend`, conectada al daemon aislado por el canal real. La etapa del Cockpit (`t_client_recv` → `t_render`, 100 ms p95) **falla en los dos modos**. No hace falta código nuevo: se verificó con el `EngineConnector` nuevo | `cargo bench -p gitraptor-cli --bench engine -- --quick --only tui-modify` (resultado en el PR) |
| (2) Cliente en `crates/api` | `gitraptor_api::client` (feature `client`): `Client`, `ClientError` (más `Launch`), `Incoming`, `Connect`, `Launch`/`NeverLaunch` y `ensure_daemon_with` (reemplazo SEC-13 y espera del handshake). Además, `client::transport` (`socket_path`, `MAX_SOCKET_PATH`, `in_dir`, `verify_private_dir`, `connect` con L-06, el nombre del pipe y `runs_as_this_user`) y `client::peer` (`PeerCred`, `peer_cred`, `current_uid`). `crates/core::client` queda como fachada: `Client` es un *newtype* con `Deref` que se abre desde `ProfileDirs`, junto con `ClientOptions::{connect, launcher}` e `InstalledLauncher`. Ningún llamador de core, cli ni mcp cambia. `channel::transport`, `channel::peer` y `profile::fsperm::verify_private_dir` delegan en api. El módulo `link` se sustituye por `client::engine::EngineConnector`, que solo usa `gitraptor_api` y recibe `Connect` y `Box<dyn Launch>` desde `commands/tui.rs` | `apps/cli/tests/tui_boundaries.rs` (sin excepción; `link.rs` no vuelve; solo el binario construye el lanzador); `crates/api/tests/client_boundary.rs` (feature, dependencias opcionales, sin core ni `directories` ni procesos, core sin segunda copia); `crates/api/tests/architecture.rs` sigue en verde |
| (3) L-06 desde la TUI | Con la carpeta del socket en 0755 y un servidor falso dentro, la TUI pasa a "Canal rechazado" y el servidor no recibe ni un byte. Con una carpeta de otro uid (`/`, de root), también "Canal rechazado". Como control, con una carpeta privada sin servidor el estado es "Motor no disponible", no "rechazado". El servidor de otro uid no se puede crear sin root: se prueba la misma comprobación con el uid esperado como parámetro (`transport::connect_expecting`), y el cliente cierra sin enviar nada | `apps/cli/tests/tui_channel_peer.rs`; `crates/api/tests/client_peer.rs` |
| (4) Suite "TUI sin perfil" | Suite `repo_intact` de INF-GRP-001 (§ 8). Con el daemon real en marcha y `data/` y `config/` del perfil en 000, una `App` sin pantalla llega a "En vivo" con la réplica global. La máquina falsa queda intacta: solo pueden cambiar los datos y el estado del motor, y las fechas de `config/`, cuyo modo cambia la prueba. Sube `repo_intact_min` en Linux (97) y macOS (104) | `apps/cli/tests/tui_without_profile.rs::repo_intact::repo_intact_the_tui_works_without_reading_the_profile` |

Siguen fuera: el **lanzador del editor** (Q-CKP-9) y Linux y Windows en máquina real (**Pendiente: etapa de validación multiplataforma**, salvo lo que diga el PR). La suite "TUI sin perfil" es solo de Unix porque Windows no tiene bits de modo que quitar.

