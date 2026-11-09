# Preparación de la corrida · US-DSK-003 (estado del motor)

Sin diseño: el plan es [DS-US-DSK-003](../requirements/features/desktop/dev-specs/US-DSK-003-estado-del-motor.md) (`spec-ready`). Esta nota recoge lo que la Dev Spec no cubre: el alcance de la corrida, la comprobación de deriva y dónde viven los tests del contrato. Preparada en el loop principal (2026-11-09), en modo rápido (sin línea base de suite completa; el resultado será SIN CERTIFICAR).

## Alcance

- **Ya hecho antes de esta corrida** (commits de la rama, tests en verde; por eso no son criterios del contrato, que exige tests en rojo): T001 (`crates/api/src/display.rs`, `visible`), las primitivas `StatusPill`, `StatePanel`, `CopyCommand` y `Skeleton`, y la parte pura de T007 (`engine-view.ts`, `EngineBand`, `EngineCenter`, `EngineStates.test.tsx`, textos en/es).
- **Por hacer** (criterios `D3-*`): T002 (daemon falso y `connection.requester`), T003 (tests en rojo), T004 (supervisor), T005 (`engine_watch`, `engine_retry`), T006 (flujo, almacén y caché) y lo que falta de T007 (`useEngine`, `TitleBar`, `AppShell`, `App`, marca de primera pintura). T008 es manual (macOS) y va en el PR.

## Comprobación de deriva

Contra `origin/main` `100a3a57`, con TS-DSK-002 (PR #334) ya dentro:

| Cita | Estado |
|---|---|
| `engine::link`: `Link`, `Connector`, `LinkError`, `ConnectFailure`, `Incompatibility`, `IncompatibilityKind`, `ClientLink`, `SocketConnector`, `map_client_error`, `REQUIRED_CAPABILITIES` | igual que la DS. `REQUIRED_CAPABILITIES` tiene tres: T002 suma la cuarta |
| `engine::launcher::BundleConnector` (`new`, `for_tests`) | igual. `bundled_raptor()` es **privada** (`fn`, sin `pub`): la DS (T005) la usa desde `lib.rs`; hay que hacerla `pub(crate)` (cambio mínimo en el fichero de TS-DSK-002, a anotar) |
| `Link::next` devuelve `(recv_ns, Notification)` | igual; el supervisor usa `recv_ns` para las marcas |
| `gitraptor_api::display::visible` | existe (T001) |
| `methods::CAP_REQUESTER` | existe (`crates/api/src/methods/connection.rs`) |

## Avisos de W137 (coordinador, 2026-11-09)

1. La app conecta **solo** por `BundleConnector`: `SocketConnector` no aplica B-3 y queda para tests. `lib.rs` no lo nombra.
2. `Incompatibility` llega cruda del daemon: `ingest` sanea `daemon_version` y `daemon_path` con `visible` antes de mandarlos. Criterio `D3-UNTRUSTED` (`an_incompatibility_from_the_daemon_is_sanitized`).

## Desviación revertida

El coordinador aprobó (2026-11-09) no tener botón "Arrancar motor" **mientras no hubiera launcher**. TS-DSK-002 (#334) ya está en `main` y `BundleConnector::connect(true)` arranca el motor, así que la premisa cayó (hallazgo PC-1 del crítico): se restaura el diseño de la DS. El estado parado y el incompatible con `action: start` muestran `engine.action.start` → `engine_retry { start: true }`; la banda de reconexión sigue con `engine.action.retryNow` → `{ start: false }`. Se quitaron el comando copiable `raptor daemon start` y la clave `engine.stopped.howTo`. Commit `1fb60e62`.

## Ajustes tras la ronda 1 del crítico (2026-11-09)

- **PC-2** · se quita `D3-GIT` (el literal `2.38` del frontend ya existe y es igual al de la TUI: nace en verde).
- **PC-3** · el tester sustituye el test de las tres capacidades de `engine_link.rs` por el de cuatro.
- **PC-4** · nuevo `D3-WIRING` (`tests/engine_wiring.rs`): `lib.rs` registra `EngineHub` con `BundleConnector` y `shutdown` en `RunEvent::Exit`; `SocketConnector` no sale de `engine/link.rs`.
- **PC-5** · nuevo `D3-CENTER` (`AppShell.test.tsx`): `EngineBand` y `EngineCenter` alimentados por `useEngine`.
- **PC-6** · la fixture la produce el tester serializando los tipos reales de `wire.rs`; `engine_wire.rs` (experto, T006) solo compara y la fixture queda congelada; si difieren, se ajusta `wire.rs`. Decisión del orquestador (2026-11-09): sustituye el "genera con `GITRAPTOR_UPDATE_FIXTURES=1`" de la DS.
- **PC-7, PC-8, PC-9** · `D3-LOST` observa la espera por `retryInMs` y el argumento `start` de `connect`; `D3-READONLY` corre el archivo entero.
- **PC-10** · la topología de la DS se corrigió (`support/mod.rs` CREATE, `launcher.rs` MODIFY, §9.4 apunta a `engine_stop_command.rs`).

## Ajustes de preparación

- **A1 · Tests propios.** `stop_command` va a `tests/engine_stop_command.rs`, no a `wire::tests`: el tester solo escribe archivos de test y la regla R4 los congela.
- **A2 · Esqueleto.** Los tipos de `wire.rs`, `SeqTrack` y `backoff` ya existen antes de los tests (los pidió T003 como stubs); las rutas de producción devuelven valores mínimos y solo `Supervisor::run` lleva `todo!()`.
- **A3 · Fixture dorada.** `tests/fixtures/engine-messages.json` la escribe el tester dentro del criterio `D3-CACHE` (la lee `engine-cache.test.ts`, que nace en rojo). `tests/engine_wire.rs` (el generador/comparador de Rust) lo escribe el experto en T006: con tipos de `wire.rs` reales nacería en verde, así que no es criterio.
- **A4 · Un solo `cargo test` por comando.** Cada criterio de Rust construye `apps/desktop/dist` antes (`pnpm -C apps/desktop run build`), como `IPC1` de TS-DSK-001.
- **A5 · Primera compilación.** El `target/` de este worktree se limpió: la primera compilación de `gitraptor-desktop` es desde cero.
