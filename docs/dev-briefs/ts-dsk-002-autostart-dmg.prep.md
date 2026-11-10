# Preparación de la corrida · TS-DSK-002, T007 a T009 (autoarranque, `.dmg` y verificación en macOS)

Sin diseño: el plan es [DS-TS-DSK-002](../requirements/features/desktop/dev-specs/TS-DSK-002-arranque-motor-binarios-bundle.md) (`spec-ready`). Esta nota recoge lo que la Dev Spec no cubre: el alcance, la deriva, las firmas que fijan los tests del contrato y los ajustes. Preparada en el loop principal (2026-11-10).

## Alcance de esta corrida

**T007, T008 y T009.** T001 a T006 entraron con #334 (`ts-dsk-002-engine-link.prep.md`). Modo rápido (Rene, 2026-11-08): prueba en rojo dirigida, `deliver-run abort` y construcción **SIN CERTIFICAR**; verificación local solo de lo afectado; la suite completa la corre el CI.

- **T007**: `tauri-plugin-dialog =2.8.3`, `engine/autostart.rs`, comandos `engine_autostart_offer` y `engine_autostart`, `commands.json` y capability sin ningún `dialog:*`, `CommandError` con `NotOffered`, `Cancelled` y `Failed`, catálogo de errores en en/es.
- **T008**: `tauri.release.conf.json`, `tools/desktop/stage-sidecars.sh`, job `desktop-app` en `release.yml` (job nuevo dentro de un workflow existente; **se avisa al coordinador en el plan**), `select-assets.sh` y su test, `.gitignore`.
- **T009**: guion de verificación manual en macOS ([`ts-dsk-002-t009-verificacion-macos.md`](ts-dsk-002-t009-verificacion-macos.md)). Lo ejecuta el coordinador; la ficha pasa a `implemented` solo con T009 verificado.

## Comprobación de deriva

Contra `6e9ffce9` (`main` + #334):

| Cita | Estado |
|---|---|
| `src/engine/{link,launcher,mod}.rs` existen; `mod.rs` no declara `autostart`; `EngineHub::new(connector, own_raptor)` (US-DSK-003 ya entró) | igual; T007 añade `pub mod autostart;` |
| `commands.json` = `app_info, engine_watch, engine_retry, fleet_open, guard_blocks`; `commands/mod.rs` los lista en `generate_handler!`; capability con `allow-<cmd>` | igual; T007 añade dos |
| `CommandError`: `InvalidParams`, `NotWatching` (`#[serde(tag="code", rename_all="kebab-case")]`) | igual |
| `tauri_surface.rs` ya lleva V2 con `tauri-plugin-dialog` permitido, `.plugin(` solo `tauri_plugin_dialog::init()` y las reglas de `tauri.release.conf.json` | igual: T007 y T008 solo tienen que cumplirlo |
| `tests/support/fake_raptor.rs`: `FakeRaptor::{printing_fixture, printing, sleeping, calls, env}` | igual |
| `raptor daemon enable` / `disable` (`DaemonAction::Enable/Disable`) | igual |
| `[dependencies]` de `gitraptor-desktop` sin `tauri-plugin-dialog` ni `nix`; `nix 0.31.3` ya está en el lock | igual |
| `.github/workflows/release.yml`: jobs `plan, scripts, build, sbom, assemble, test-installers, draft-release`; no hay `desktop-app` | igual |
| `tools/desktop/` no existe | igual |

## Firmas que fijan los tests del contrato

La Dev Spec da nombres y comportamiento; estas son las firmas concretas para poder probar sin diálogo ni `HOME` real (no cambian ningún nombre de la DS).

```rust
// apps/desktop/src-tauri/src/engine/autostart.rs
pub enum NotOfferedReason { Location, OtherBinary }
pub enum Offer { Offered, Registered, NotOffered { reason: NotOfferedReason } }
/// Regla pura: `home` es el home del usuario (el comando lo saca de nix::unistd::User::from_uid, nunca de HOME).
pub fn offer_with_home(app_exe: &Path, home: &Path, report: &gitraptor_api::client::ensure::EnsureReport) -> Offer;
/// `offer_with_home` compara las rutas TAL CUAL (sin tocar el disco ni canonicalizar): sus tests usan rutas que no existen.
/// `offer` es quien canonicaliza `app_exe` y el home antes de llamarla.
pub fn offer(app_exe: &Path, report: &EnsureReport) -> Offer; // home = nix::unistd::User::from_uid(getuid), nunca HOME
pub struct Prompt { pub title: String, pub body: String, pub accept: String, pub cancel: String }
pub enum Lang { En, Es }
/// Pura: `Es` si el primer idioma de la salida de `defaults read -g AppleLanguages` empieza por "es"; si no, `En`.
pub fn lang_from_apple_languages(output: &str) -> Lang;
/// Ejecuta `/usr/bin/defaults read -g AppleLanguages` (argv fijo, entorno solo PATH, tope 3 s); cualquier fallo es `En`.
pub fn system_lang() -> Lang;
pub fn prompt(enable: bool, lang: Lang) -> Prompt; // textos de la DS § T007
/// Pide la confirmación con `confirm` (en producción, el diálogo nativo) y, con "Sí", ejecuta
/// `<raptor> daemon enable|disable` con argv fijo, entorno = solo PATH, cwd "/", stdin/stderr nulos
/// y tope de 10 s. Devuelve `registered`. No ejecuta nada si `confirm` devuelve false.
pub fn run_choice(raptor: &Path, enable: bool, offer: &Offer, lang: Lang, confirm: &mut dyn FnMut(&Prompt) -> bool) -> Result<bool, crate::ipc::CommandError>;
```

- `enable = true` exige `Offer::Offered`; `enable = false` exige `Offer::Registered`; otra combinación es `CommandError::NotOffered` sin abrir el diálogo ni ejecutar nada.
- `confirm` devuelve `false` → `CommandError::Cancelled`, sin ejecutar `raptor`.
- Salida distinta de 0, tiempo agotado o `raptor` inejecutable → `CommandError::Failed`.
- `commands::engine_autostart::EngineAutostartParams { enable: bool }` (`deny_unknown_fields`, `camelCase`); `engine_autostart_offer` recibe `{}`.
- Respuestas: `engine_autostart_offer` → `{ "offer": "offered"|"registered"|"not-offered", "reason": "location"|"other-binary"|null }`; `engine_autostart` → `{ "registered": bool }`.

Frontend (decisión de este plan, ver D-A):

```ts
// apps/desktop/src/engine/AutostartOffer.tsx — puro: props dentro, callbacks fuera
export interface AutostartOfferProps { offer: "offered" | "registered"; pending?: boolean; onChange: (enable: boolean) => void }
```

`commands.ts` gana `engine_autostart_offer` y `engine_autostart`; los errores `not-offered`, `cancelled` y `failed` se traducen con `error.<code>`.

## Decisiones y ajustes

- **D-A · Un punto de entrada en la UI (decisión del orquestador, 2026-11-10; el aprobador la confirma en la firma).** La DS fija solo backend, `commands.ts` e i18n de errores (`frontend_surface: false`), pero T009 paso 3 exige que el humano pueda pedir el autoarranque desde la app instalada y la release no lleva devtools: no hay otro medio. Se añade el componente puro `AutostartOffer` (primitivas de `src/ui` y tokens, layout B1, en/es, axe, claro y oscuro) y el contenedor `apps/desktop/src/engine/AutostartSection.tsx`, que `src/shell/AppShell.tsx` monta bajo `EngineBand`: consulta `engine_autostart_offer` solo con el motor conectado y muestra `AutostartOffer` solo con `offer ∈ {offered, registered}`; al pulsar llama a `engine_autostart` con `{ enable }` y traduce los errores `not-offered`, `cancelled` y `failed` con `error.<code>`. El diálogo sigue siendo del backend.
- **D-B · Idioma del diálogo (decisión del orquestador).** El backend lo decide por el idioma del SO sin leer el entorno (V7) y sin dependencias nuevas: `system_lang()` ejecuta `/usr/bin/defaults read -g AppleLanguages` con argv fijo y entorno solo `PATH`, y `lang_from_apple_languages` (pura, probada) elige `Es` o `En`; ante cualquier fallo, `En`. El webview nunca da textos ni idioma.
- **D-C · Home sin `HOME` y sin `unsafe` (decisión del orquestador).** `offer` obtiene el home con `nix::unistd::User::from_uid(nix::unistd::getuid())` (`nix =0.31.3`, feature `user`, bajo `cfg(unix)` en `gitraptor-desktop`; ya está en `Cargo.lock` y la usa `gitraptor-core`). La FFI queda dentro de `nix`; `gitraptor-desktop` conserva `unsafe_code = "forbid"` y V1 no se toca (`nix` no es `gitraptor-core/-git/-policy`). Si no se resuelve el home, solo se ofrece `/Applications/`.
- **D-D · `desktop-app` y `release.yml`.** Es un job nuevo en un workflow existente; no cambia variables `RUST_*` ni `CARGO_*`. Sin secretos (`signingIdentity: "-"`), `permissions: contents: read`. Se avisa al coordinador en el plan, como pidió.
- **D-E · Verificación del workflow.** Un test estático (`tools/desktop/release-job.test.sh`) comprueba la forma del job; el build real corre en el CI del PR (no se lanza `workflow_dispatch` en macOS).

## Gaps y supuestos

- El build de `tauri build --bundles dmg` no corre en local en esta corrida (sin firmar y lento); lo prueba el job `desktop-app` del CI del PR y T009.
- `tauri-plugin-dialog` añade crates al `Cargo.lock`: se deja que cargo añada lo mínimo.
- Windows y Linux: fuera (etapa de validación multiplataforma). Solo macOS aarch64.

## Convenciones observadas

- Tests del contrato en `tests/*.rs` (integración) y `src/**/*.test.tsx` (vitest); cada `verify` de Tauri construye antes el frontend (`pnpm -C apps/desktop run build`).
- Los tests de la app usan `FakeRaptor` de `tests/support/fake_raptor.rs`; la lista de comandos se comprueba por igualdad con `commands.json` (`tests/commands.rs`, `src/client/commands.test.ts`).

## Inventario de reutilización

- Reutiliza: `launcher::run_ensure`/`bundled_raptor`, `EnsureReport` (`autostart.registered`, `binary`), `ipc::{params, CommandError}`, `FakeRaptor`, primitivas `Button`/`Toggle`/`Pill` de `src/ui`, `tauri_surface.rs` (V2).
- Crea nuevo: lo que lista la DS en T007/T008, más `AutostartOffer` (D-A) y los dos scripts de prueba de `tools/desktop/`.

## Notas del crítico (ronda 1)

- **PC-1 · Checksums del `.dmg`.** `assemble` calcula hoy `.sha256` y `SHA256SUMS` solo de `raptor-*` (`release.yml`, paso Checksums; `select-assets.sh` también). T008 los extiende a `GitRaptor-*.dmg`: `assemble` hace hash del `.dmg`; `select-assets.sh` copia el `.dmg` y su `.sha256` y añade su línea a `SHA256SUMS` de la alpha. `release-job.test.sh` comprueba además el renombrado del `.dmg` y el `upload-artifact` `archive-desktop-aarch64-apple-darwin`.
- **PC-4 · Guion T009.** Lo escribe esta corrida (`ts-dsk-002-t009-verificacion-macos.md`) y lo observa `tools/desktop/t009-script.test.sh` (existe, cuatro pasos de la DS, el paso 3 pasa por la fila de `AutostartSection`).
- **PC-8 · `DSK002-BOUNDARY`** es una guarda de regresión que ya está en verde (`boundaries.rs`, `environment.rs`); no es prueba en rojo de T007. «Sin `unsafe`» lo garantiza el lint `unsafe_code = "forbid"` del workspace.
- **Clave i18n de los errores.** El test FE3 del catálogo exige claves lowerCamel: el código `not-offered` del canal se traduce con `error.notOffered` (ídem `cancelled`, `failed`), siguiendo el mapeo que ya usan los demás errores; los tests del contrato aceptan ambas formas y FE3 decide.
- **Rojo por compilación.** Los tests Rust nuevos fallan hoy por símbolo ausente (`engine::autostart`, `CommandError::{NotOffered,Cancelled,Failed}`); en modo rápido no hay línea base B3 y se acepta como rojo válido (sin stubs).

## Condiciones de la firma (coordinador, 2026-11-10)

- `release.yml` solo corre con tags `v*`: **no** se añade `pull_request` a `release.yml` ni un job de macOS al CI de PR. El CI del PR no construye el `.dmg`; el primer run real del job `desktop-app` es la alpha del viernes. El guion T009 incluye construir el `.dmg` en local con el mismo script que usa el job (`stage-sidecars.sh` y `tauri build --bundles dmg`), que ejecuta el coordinador. Se deja escrito en el PR.
- D-A, D-B y D-C aceptadas. PC-10 aplicado (comentario de `rustix`).
