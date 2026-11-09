# Preparación de la corrida · TS-DSK-002, T001 a T006 (enlace de la app con el motor)

Sin diseño: el plan es [DS-TS-DSK-002](../requirements/features/desktop/dev-specs/TS-DSK-002-arranque-motor-binarios-bundle.md) (`spec-ready`). Esta nota solo recoge lo que la Dev Spec no cubre: el alcance de este PR, la comprobación de deriva y dónde viven los tests del contrato. Preparada en el loop principal (2026-11-09).

## Alcance de esta corrida

Solo **T001 a T006** (decisión del coordinador, 2026-11-09): `EnsureReport`, `ReplacePolicy`, la ruta del ejecutable de un pid y B-3, `raptor daemon ensure`, la costura `engine::link` y `BundleConnector`. Fuera: T007 (diálogo y comandos de autoarranque), T008 (`.dmg`) y T009 (verificación manual). Por eso `engine/mod.rs` declara solo `pub mod link; pub mod launcher;` (sin `autostart`), y no se añade ningún comando IPC, plugin ni permiso.

## Comprobación de deriva

Las citas de la Dev Spec, contra `f752e063` (`origin/main`):

| Cita | Estado |
|---|---|
| `crates/api/src/client/mod.rs`: `ClientError` (12 variantes), `Connect`, `Launch`, `ensure_daemon_with`, `replace_same_protocol`, `replace`, `Client::connect_runtime(…, verify: impl FnOnce(u32) -> bool)`, `missing_capabilities`, `next_incoming` → `Incoming` | igual |
| `crates/api/src/client/peer.rs`: `peer_cred`, `current_uid` | igual |
| `crates/api/Cargo.toml`: feature `client` sin `gitraptor-macsys` | igual (T003 la añade) |
| `crates/macsys`: `process.rs` público, FFI en `ffi_*` privados; `unsafe_boundary` cubre `crates/macsys` | igual |
| `crates/core/src/autostart.rs`: `refuse_transient(exe, temp: &[PathBuf])`, `Autostart::for_current_user() -> Option<Self>`, `is_registered`, `registered_exe` | igual. En debug con `GITRAPTOR_PROFILE_DIR` y sin `GITRAPTOR_AUTOSTART_DIR`, `for_current_user` da `None`: `autostart = { registered: false, binary: null }` en los tests |
| `crates/core/src/client.rs`: `ClientOptions { capabilities: Vec<&'static str> }`, `connect()`, `launcher()` | igual, con el ajuste A1 |
| `apps/cli/src/commands/daemon.rs`: `DaemonAction` con `Stop`, `Status`, `Enable`, `Disable`; `support::profile_dirs` | igual |
| `apps/desktop/src-tauri`: `lib.rs` con `commands`, `ipc`, `window`; V1 (`tests/boundaries.rs`) prohíbe `gitraptor-core`, `-git`, `-policy` (no `-macsys`); V7 (`tests/environment.rs`) prohíbe `env::var`, `var_os`, `env::args`, `args_os` en `src/` | igual. `current_exe`, `env_clear` y `Command::env` no los toca V7 |
| `gitraptor_api::methods::{CAP_SCOPE_ACTIVITY, CAP_OBSERVATION_TIERS, CAP_PREDICTION_PAIRS}` (`Capability`, campo `.name`) | igual; `REQUIRED_CAPABILITIES` usa `.name` |

## Ajustes de preparación (no cambian nombres ni firmas de la DS)

- **A1 · `--require` con `ClientOptions`.** `ClientOptions.capabilities` es `Vec<&'static str>` y `--require` llega como `String`. La CLI usa el `&'static str` de `capability::all()` cuando el nombre es conocido y, si no lo es (el caso de prueba «capacidad inexistente»), `String::leak` (proceso de un solo uso, a lo sumo unos pocos nombres).
- **A2 · Enlace simbólico en `is_installed_raptor`.** La DS pide `symlink_metadata` sobre la ruta canónica y, a la vez, rechazar un enlace simbólico; canonicalizar sigue el enlace. Se comprueba primero `symlink_metadata` sobre la ruta recibida (un enlace se rechaza) y después nombre, tipo, dueño y modo sobre la canónica. `bundled_raptor` ya canonicaliza; `/proc/<pid>/exe` llega resuelto.
- **A3 · Tests en archivo propio.** La aceptación de T003 en `client_peer.rs` va a `crates/api/tests/client_peer_installed.rs`, y la de `gitraptor-macsys` a `crates/macsys/tests/executable_path.rs`: los tests del contrato no se mezclan con los existentes (regla R4).
- **A4 · Socket de prueba propio.** El daemon falso de US-DSK-003 no existe en `main`: `tests/support/fake_raptor.rs` levanta un servidor de prueba en una carpeta temporal 0700 que responde al `connection.hello`.
- **A5 · Fixtures fuera de `FIXED_TEMP`.** El test que acepta un `raptor` propio crea su carpeta con `tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))`, porque en macOS `tempdir()` cae en `/var/folders` (dentro de `FIXED_TEMP`).

## Ajustes tras la ronda 1 del crítico (2026-11-09)

- **PC-1 · B-3 positivo.** DSK002-B3 también lanza una copia 0755 llamada `raptor` en `CARGO_TARGET_TMPDIR` y exige `installed_raptor_peer(pid) == true`.
- **PC-2 · «Otra ruta» sin `daemon.replace`.** DSK002-ENSURE-CLI usa servidores de prueba en el runtime del perfil temporal que registran los métodos recibidos: uno de protocolo anterior (`incompatible daemon-older`, salida 3) y uno del mismo protocolo sin una capacidad pedida (`incompatible missing-capabilities`); ninguno recibe `daemon.replace`. El daemon real ya rechaza un reemplazo que no viene del binario instalado, así que solo un registro observable prueba que la CLI pasa `ReplacePolicy::OnlyIf`.
- **PC-3 · Rutas transitorias reales.** Los rechazos por `node_modules`, `_npx` y `/tmp` son archivos `raptor` 0755 reales; si no, la comprobación de A2 los rechazaría sin pasar por `is_transient`.
- **PC-4 · Fronteras.** Nuevo criterio DSK002-BOUNDARY: V1, V2 y V7 (`boundaries`, `environment`, `tauri_surface`), porque la suite por defecto excluye `gitraptor-desktop`.
- **PC-5 · «Solo PATH».** El registro del `raptor` falso ignora `PWD`, `SHLVL`, `_` y `OLDPWD`, que crea el propio `/bin/sh`.

## Gaps y supuestos

- **Topología extra (PC-6).** `apps/desktop/src-tauri/Cargo.toml` (MODIFY): `tempfile` en `[dev-dependencies]`, para `fake_raptor.rs` y el socket de prueba; la DS solo lo listaba en T007.
- **`bundled_raptor()` positivo (PC-7).** Ningún criterio de esta corrida prueba que `BundleConnector::new()` ejecute un `raptor` hermano válido: los tests no pueden colocar un `raptor` junto al binario de test sin tocar `target/`. Lo cubre T009 (verificación con la app instalada), en el PR que cierre TS-DSK-002. El experto puede añadir un test unitario de la regla de resolución en `src/` con una ruta inyectada.

## Convenciones observadas (lo que la Dev Spec no fija)

- Tests del contrato de integración (`tests/*.rs`), escritos por el criterion-tester sin tocar `src/`; el experto puede añadir tests unitarios en `src/`.
- Los tests del crate de Tauri necesitan `apps/desktop/dist`: cada `verify` construye antes el frontend (`pnpm -C apps/desktop run build`), como en TS-DSK-001.
- Los tests de la CLI usan perfiles temporales con `GITRAPTOR_PROFILE_DIR` (solo en debug) y el binario `CARGO_BIN_EXE_raptor`, como `apps/cli/tests/daemon_process.rs`. «Un daemon de otra ruta» es una copia de ese binario en una carpeta temporal.

## Inventario de reutilización

- Reutiliza: `ensure_daemon_with` (pasa a `ensure_daemon_policy` + `ReplacePolicy::Any`), `refuse_transient` (pasa a llamar a `installed::is_transient`), `peer::peer_cred`, `Client::connect_runtime`, `ClientOptions::{connect, launcher}`, `Autostart::for_current_user`.
- Crea nuevo: lo que lista la DS en § Estructura de ficheros para T001-T006.

## Modo rápido (Rene, 2026-11-08)

Sin línea base de suite completa (bug de P3 en nassa-core): tras la firma, prueba dirigida de que los tests del contrato fallan por la razón correcta, `deliver-run abort` y construcción **SIN CERTIFICAR**. Verificación local: tests del contrato, `cargo test -p` de cada crate tocado, `clippy --workspace --all-targets -D warnings` y `fmt --check`; la suite completa la corre el CI.
