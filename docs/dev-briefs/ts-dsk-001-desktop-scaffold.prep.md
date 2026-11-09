# Preparación de la corrida · TS-DSK-001 (andamiaje de `apps/desktop`)

Sin diseño: el plan es [DS-TS-DSK-001](../requirements/features/desktop/dev-specs/TS-DSK-001-andamiaje-app-escritorio.md) (`spec-ready`). Esta nota solo recoge lo que la Dev Spec no cubre: la comprobación de deriva, las versiones verificadas y dónde viven los tests del contrato. Preparada en el loop principal (2026-11-08).

## Comprobación de deriva

Las citas de `must_read[]` siguen ciertas en `0f43647e` (`origin/main`):

| Cita | Estado |
|---|---|
| `Cargo.toml`: `members = ["crates/*", "apps/cli", "apps/mcp"]`, sin `default-members` | igual |
| `crates/api/Cargo.toml`: feature `client`; `crates/api/src/lib.rs`: `pub mod client` tras `cfg(feature = "client")`, `PROTOCOL_VERSION: u32` | igual |
| `packages/design-tokens`: `build.mjs`, `rust-format.mjs`, `test/tokens.test.mjs`, `node --test` | igual |
| `tools/ci/cargo-test-selection.sh` emite `--workspace`; `tools/test/nextest-junit.mjs` usa `--workspace` por defecto; `repo-intact.yml` filtra `apps/**` | igual |
| `rust-toolchain.toml` 1.99 ≥ `rust-version` 1.90 de tauri 2.12.5 | compatible |

## Versiones y licencias (comprobadas el 2026-11-08 en npm y crates.io)

Todas existen y tienen 14 días o más publicadas. `tauri` 2.12.5 y `tauri-build` 2.7.4: Apache-2.0 OR MIT. npm: MIT, salvo `@tauri-apps/api` y `@tauri-apps/cli` (Apache-2.0 OR MIT) y `axe-core` 4.14.0 (MPL-2.0, solo `devDependencies`: no se distribuye en el bundle). El check `license declared` solo mira la licencia que declara cada paquete propio: `apps/desktop/package.json` y el crate declaran `FSL-1.1-ALv2`.

## Convenciones observadas (lo que la Dev Spec no fija)

- **Tests del contrato en archivo propio** (`tests/*.rs` de integración, no `#[cfg(test)]` dentro de `src/`), para que el criterion-tester los escriba sin tocar código de producción. Por eso `window` e `ipc` se prueban en `apps/desktop/src-tauri/tests/window.rs` y `tests/commands.rs` sobre `gitraptor_desktop::window` y `gitraptor_desktop::ipc` (módulos `pub`). Es el patrón de `apps/cli/tests/tui_boundaries.rs`. El experto puede añadir tests unitarios en `src/`.
- **Verificación de CI como test**: la aceptación de T008 vive en `tools/ci/desktop-exclusion.test.mjs` (`node --test`, como `tools/test/nextest-junit.test.mjs`).
- **`generate_context!` necesita `apps/desktop/dist`**: cada `verify` de Rust construye antes el frontend.

## Inventario de reutilización

- Reutiliza: `gitraptor_api::PROTOCOL_VERSION` (`crates/api/src/lib.rs`); el patrón de `cargo metadata` de `apps/cli/tests/tui_boundaries.rs`; el pipeline de Style Dictionary de `packages/design-tokens/scripts/build.mjs`; las acciones y SHA de `tokens.yml` y `repo-intact.yml`.
- Genera: los iconos con `tauri icon`; `css/glass.css` con `@gitraptor/tokens build`.
- Crea nuevo: lo que lista la Dev Spec en § Estructura de ficheros.

## Ajustes tras la ronda 1 del crítico (2026-11-08)

- **Módulos públicos.** `lib.rs` expone `pub mod window; pub mod ipc; pub mod commands;` (y `commands::app_info` es `pub`), porque los tests del contrato son de integración. La aceptación de T001 y T002 que cita `--lib window` y `--lib ipc` se cumple con `--test window` y `--test commands`; el experto puede añadir tests unitarios en `src/`.
- **`app_info` sin `Request` en los tests.** `tauri::ipc::Request` no se construye fuera del runtime. El comando delega en una función pura `pub fn info() -> AppInfo` (en `src/commands/app_info.rs`) y `tests/commands.rs` prueba `ipc::parse` (con cuerpos que no son objeto) e `info()`. No se añade la feature `test` de `tauri`. El caso «cuerpo no JSON» (PC-7, ronda 2): `ipc::params(request)` delega en la función pura `pub fn from_body<P: DeserializeOwned>(body: &tauri::ipc::InvokeBody) -> Result<P, CommandError>` (`InvokeBody` es un enum público y se construye en un test), que rechaza `InvokeBody::Raw` con `invalid-params` y delega `Json` en `parse`; `tests/commands.rs` lo prueba (IPC1).
- **STA1.** Lock congelado, lint, typecheck y clippy del crate, como criterio estático.
- **TOK1** comprueba además que `crates/theme/src/generated.rs` no cambia al construir. **V2** compara también la `devCsp` exacta. La suite corre también los tests de `tools/ci` y `tools/test`.
