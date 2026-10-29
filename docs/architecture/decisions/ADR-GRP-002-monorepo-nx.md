---
id: ADR-GRP-002
title: Monorepo políglota con Nx (package-based), pnpm y Cargo workspaces
type: adr
status: accepted
date: 2026-10-01
created: 2026-10-01
updated: 2026-10-28
deciders: [Rene Bonilla]
related: [BRD-GRP-001, ADR-GRP-001, ADR-GRP-003, ADR-GRP-007, ADR-GRP-009, ADR-GRP-017, INF-GRP-001]
changelog:
  - 2026-10-04: crates/testkit (INF-GRP-001).
  - 2026-10-05: crates/winsys.
  - 2026-10-07: crates/macsys; nx sobre cargo y CI con affected.
  - 2026-10-28: apps/desktop entra en el MVP (D7); paquetes web y CI de la app.
tags: [nx, monorepo, package-based, pnpm, cargo, rust, monodon, tauri, ci]
---

# ADR-GRP-002 — Monorepo con Nx (package-based)

## Contexto

GitRaptor es políglota (ADR-GRP-001):
- Rust para el motor, la CLI/TUI, el MCP y el backend de Tauri.
- TypeScript/React para la app de escritorio, el UI kit y la extensión de VS Code/Cursor.

Necesitamos:
- Compartir código entre apps, como el UI kit (ADR-GRP-003) y los tipos del motor.
- Builds y tests incrementales con caché.
- Un grafo de dependencias entre proyectos.
- Releases coordinados de binarios, VSIX y crates.

## Decisión

Usar **Nx en modo package-based**:

- **JS/TS:** **pnpm workspaces**. Cada paquete tiene su `package.json` con sus propios scripts, que Nx infiere como targets. Las dependencias internas van con `workspace:*`.
- **Rust:** un **Cargo workspace** en la raíz (`Cargo.toml` con `[workspace]`, `members` y `[workspace.dependencies]`), integrado a Nx con el plugin **`@monodon/rust`** (v3.x). El plugin expone `build`, `test`, `lint` (clippy) y `run`, y construye el grafo a partir de las dependencias `path`.
- **Tauri:** el frontend es un paquete normal de pnpm. `src-tauri` es miembro del Cargo workspace. `tauri dev` y `tauri build` corren como scripts de `package.json`.
- **Caché:** activada en `nx.json` (`targetDefaults` con `cache: true` para build, test y lint). Un `namedInput` `rust` cubre `Cargo.toml`, `Cargo.lock` y `rust-toolchain.toml`.
- **CI:** `nx affected` para correr solo lo que cambió. Nx Cloud o una caché remota son opcionales.
- **Fronteras:** tags de Nx (`scope:*`, `type:*`) y la regla `@nx/enforce-module-boundaries` para evitar dependencias indebidas. Por ejemplo, `ui-kit` no puede depender de apps.

## Estructura propuesta

```
gitraptor/
├─ nx.json
├─ package.json            # scripts raíz, devDeps (nx, @monodon/rust, typescript)
├─ pnpm-workspace.yaml     # apps/*, packages/*
├─ Cargo.toml              # [workspace] members = ["crates/*", "apps/*/src-tauri", "apps/cli", "apps/mcp"]
├─ rust-toolchain.toml
├─ apps/
│  ├─ cli/                 # Rust · binario `raptor` (CLI + TUI con ratatui)
│  ├─ mcp/                 # Rust · binario `raptor-mcp` (rmcp)
│  ├─ desktop/             # React + Vite (frontend Tauri)
│  │  └─ src-tauri/        # Rust · backend Tauri → usa crates/core
│  └─ vscode-extension/    # TS · cliente liviano del motor (Fase 3)
├─ crates/
│  ├─ core/                # Rust · motor: watcher, oplog/snapshots, conflictos
│  ├─ policy/              # Rust · configuración en tres niveles (.gitraptor/settings.json, ADR-GRP-007) y motor de guardrails
│  ├─ git/                 # Rust · capa Git (gitoxide para leer + Git CLI para escribir)
│  ├─ api/                 # Rust · contrato JSON-RPC/eventos; genera tipos TS (ts-rs/specta)
│  ├─ theme/               # Rust · paleta generada desde @gitraptor/tokens para TUI/CLI
│  └─ testkit/             # Rust · solo pruebas: arnés "repo intacto" (INF-GRP-001); solo dev-dependency
├─ packages/
│  ├─ ui-kit/              # React · @gitraptor/ui (ADR-GRP-003)
│  ├─ design-tokens/       # @gitraptor/tokens · colores, tipografía, espaciado, temas
│  ├─ graph-renderer/      # @gitraptor/graph · grafo Git en Canvas/WebGL
│  └─ core-client/         # @gitraptor/client · cliente TS tipado del motor (tipos de crates/api)
└─ docs/
```

**Creación por fase:**
- **MVP (Fase 1):** `apps/cli`, `apps/mcp`, `crates/{core,policy,git,api,theme}`, `packages/design-tokens` y el crate de soporte de pruebas `crates/testkit` (enmienda 2026-10-04).
- **Fase 3:** `apps/desktop`, `apps/vscode-extension`, `packages/{ui-kit,graph-renderer,core-client}`. No se crean antes para no cargar el monorepo con paquetes vacíos. **(Enmienda 2026-10-28, D7)**: `apps/desktop` se crea ya; los paquetes web se extraen según la sección final.

## Alternativas consideradas

- **Nx integrated (un solo `package.json`):** da más control centralizado, pero encaja peor con un repo políglota y con publicar paquetes independientes (VSIX, crates). Se descarta a favor de package-based, a pedido del equipo.
- **Turborepo:** liviano, pero sin soporte de primera clase para Rust ni grafo de proyectos políglota.
- **Solo Cargo workspace + pnpm sin orquestador:** sin caché ni `affected`, y la CI escala peor.
- **Moon / Bazel:** potentes para políglota, pero con curva y operación mayores para un equipo pequeño.

## Consecuencias

- ✅ Un solo repo, con caché y `affected` para Rust y TS, y el grafo de dependencias visible (`nx graph`).
- ✅ El UI kit y el cliente tipado se reutilizan entre la app de escritorio y la extensión.
- ⚠️ `@monodon/rust` es un plugin **comunitario**. **Mitigación:** fijar la versión. Si falla, el plan B son targets `nx:run-commands` sobre `cargo`.
- ⚠️ Cargo comparte el directorio `target/`. **Mitigación:** un `target-dir` distinto por target cuando haga falta, para evitar que los outputs cacheados se pisen.
- ⚠️ Nx Release para crates de Rust puede necesitar versionado legacy con `@monodon/rust`. **Mitigación:** validarlo al configurar los releases. La distribución principal son binarios y VSIX, no crates.io.
- ⚠️ No hay plugin oficial de Nx para Tauri. **Mitigación:** scripts de `package.json` (`tauri dev` / `tauri build`) como targets inferidos.

## Referencias

- [BRD-GRP-001 — Documento de negocio de GitRaptor](../../business/gitraptor-documento-de-negocio.md)
- [Nx — Add a Rust application to an Nx workspace](https://nx.dev/docs/kb/add-rust-to-nx-workspace)
- [Nx blog — Polyglot monorepos with Nx, TanStack and Rust](https://nx.dev/blog/polyglot-nx-monorepo-rust-tanstack)
- [@monodon/rust (npm)](https://www.npmjs.com/package/@monodon/rust)
- [Nx Release with Rust](https://nx.dev/docs/guides/nx-release/publish-rust-crates)
- [Tauri — Monorepo integration discussion #7368](https://github.com/orgs/tauri-apps/discussions/7368)

---

Enmienda 2026-10-03: referencias a policy.yaml sustituidas por ADR-GRP-007 (configuración en tres niveles).

## Enmienda (2026-10-04, INF-GRP-001)

Decisión del orquestador (2026-10-04), validada por el Arquitecto:
- Se añade **`crates/testkit`** (`gitraptor-testkit`, `publish = false`), el arnés "repo intacto" de INF-GRP-001 (huella, ejecución de control, guarda, repo canario y auditoría de `exec`).
- **No es producto**: ningún paquete puede depender de él salvo como `[dev-dependencies]`. Lo comprueba `crates/testkit/tests/dev_only.rs` sobre `cargo metadata`.
- **No depende de ningún crate de GitRaptor**, para que los tests de un crate no enlacen dos copias de ese crate.
- En Nx tiene la etiqueta `type:test-support`.
- El gate de CI del arnés ejecuta siempre todo el workspace (`cargo test --workspace -- repo_intact`), nunca `nx affected`.

## Enmienda (2026-10-05, crates/winsys)

Decisión del orquestador (2026-10-05), validada por el Arquitecto y por security-expert, y acordada con la rama `win-acl` (#72). Las dos ramas se unificaron en un solo crate al rebasar sobre `main`:
- Se añade **`crates/winsys`** (`gitraptor-winsys`, `publish = false`, etiqueta Nx `type:engine`): las llamadas a Win32 que necesita el resto del workspace, detrás de una API segura. Es **un solo crate** con tres módulos públicos: `acl` (dueño y DACL del perfil y de `git.exe`, [TD-GRP-001](../../requirements/features/motor-local/technical-stories/TD-GRP-001-acl-windows.md), ADR-GRP-006 y ADR-GRP-009), y `process` y `system` (resolución del solicitante, [ADR-GRP-005, Enmienda 2026-10-05](ADR-GRP-005-forma-motor-proceso-segundo-plano.md#enmienda-2026-10-05-windows-procesos)). Sus módulos FFI privados son `ffi_acl`, `ffi_process` y `ffi_handle`. Este último es el único dueño de los handles del kernel: nunca envuelve un centinela (NULL o `INVALID_HANDLE_VALUE`) y es el único `CloseHandle` del crate. No es `Send`; si algún día tiene que cruzar hilos, el `unsafe impl Send` irá en `ffi_handle` y pasará por revisión de seguridad. Comparten una sola versión de `windows-sys` y una sola lista de features.
- **Es la única excepción a `unsafe_code = "forbid"`**. No hereda `[lints] workspace = true`: declara `unsafe_code = "deny"`, y el `unsafe` solo se permite en sus módulos privados `ffi_*` (un `#[allow(unsafe_code)]` sobre cada uno, en `lib.rs`). También tiene en deny `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks` y `clippy::multiple_unsafe_ops_per_block`. Cada bloque hace una llamada y explica por qué es seguro.
- **Función de fitness**: `crates/winsys/tests/unsafe_boundary.rs` comprueba sobre `cargo metadata` que todos los demás paquetes declaran `[lints] workspace = true`, que el workspace mantiene `forbid` y que fuera de los módulos `ffi_*` (en cualquier subcarpeta de `src/`) no hay `unsafe`.
- `windows-sys` va en `[workspace.dependencies]` (`0.61`, la versión que ya trae gix) y solo lo usa este crate. Se descartan `sysinfo` (hora de inicio en segundos) y `wmi` (COM, lento).
- Todo cambio en un módulo `ffi*` pasa por una revisión de seguridad.

## Enmienda (2026-10-07, crates/macsys)

Decisión del orquestador (2026-10-07), validada por el Arquitecto (`nassa-architect:architect`, una consulta; sus ajustes ya están incorporados). La pide la segunda línea frente a `git commit --no-verify` ([DS-US-GRD-018](../../requirements/features/guardrails/dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md), D6 y § 11): para clasificar el subcomando del `git` antecesor hay que leer la línea de órdenes de otro proceso, y en macOS eso es `sysctl(KERN_PROCARGS2)`, que ninguna dependencia segura envuelve.

- Se añade **`crates/macsys`** (`gitraptor-macsys`, `publish = false`, etiqueta Nx `type:engine`): las llamadas al sistema de macOS que necesita el resto del workspace, detrás de una API segura. Hoy tiene un módulo público, `process` (`process_args(pid)`: solo `argc` y `argv`, nunca el entorno, con un búfer acotado a 4 MiB; cualquier error, también `EINVAL` o `EPERM` de otro usuario, es `None`), y un módulo FFI privado, `ffi_procargs`. `libc` va en `[workspace.dependencies]` con la versión que ya tenía el lock (`0.2.190`) y solo como dependencia de `cfg(target_os = "macos")`.
- **Crate hermano, no un crate de sistema común.** Se descarta renombrar `winsys` a un crate por SO: obliga a tocar todas las rutas de Windows y mezcla dos superficies que se revisan por separado.
- **Con él, las excepciones a `unsafe_code = "forbid"` son dos**: `winsys` y `macsys`, con las mismas reglas: `unsafe_code = "deny"`, `unsafe` solo en módulos privados `ffi_*` (un `#[allow(unsafe_code)]` sobre cada uno, en `lib.rs`), `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks` y `clippy::multiple_unsafe_ops_per_block` en deny, un bloque por llamada con su comentario `SAFETY`, y revisión de seguridad en todo cambio de un módulo `ffi*`.
- **Una sola lista de excepciones**: `crates/winsys/tests/unsafe_boundary.rs` la declara (`EXCEPTIONS`) y comprueba, para cada una, sus lints, que `lib.rs` niega `unsafe` y que fuera de sus `ffi_*` no hay `unsafe`; y, para el resto del workspace, que hereda `[lints] workspace = true`.
- Linux no necesita crate: lee `/proc/<pid>/cmdline` sin `unsafe` (`crates/core/src/channel/peer.rs`). Windows queda sin leer (`None`, se evalúa): **Pendiente: etapa de validación multiplataforma** (`NtQueryInformationProcess`, en `winsys`).
- **Registro (2026-10-08, CPU en reposo, RES-01):** `macsys` suma `process::user_processes(uid)` y el módulo FFI privado `ffi_kinfo`, con `sysctl(KERN_PROC_UID)`. Es una sola llamada para toda la tabla de procesos del usuario, en lugar de un `proc_pidinfo` por proceso en cada escaneo S1 del detector (ADR-GRP-012). Filtra por uid efectivo, igual que `proc_listpids(PROC_UID_ONLY)`.
- **Registro (2026-10-08, churn en carpetas ignoradas, RES-01):** `macsys` suma el módulo público `fsevents` (flujo de FSEvents con rutas de exclusión y `since_when`) y el módulo FFI privado `ffi_fsevents` (CoreServices, CoreFoundation y `libdispatch`, declarados a mano). Sustituye a `notify` solo en macOS. Es la tercera superficie `unsafe` de `macsys`; sigue las mismas reglas y pasa por revisión de seguridad. Ver ADR-GRP-010, Enmienda 2026-10-08.
  - **Offsets de `struct kinfo_proc` escritos a mano**, porque `libc` no la declara: pid, ppid, inicio y `p_stat`, con un registro de 648 bytes. Se comprobaron con `offsetof` en arm64 y x86_64, y el módulo solo compila en esas dos arquitecturas.
  - **Parseo en una función pura** (`parse_kinfo`), probada en todos los SO. Descarta los zombis.
  - **Búfer acotado**: como mucho 16 MiB y 3 reintentos ante `ENOMEM`.
  - **Comprobación en ejecución**: el propio proceso debe aparecer con su padre. Si falla, el detector vuelve a `proc_pidinfo`, de modo que un fallo cuesta CPU pero no deja ciega la detección.
  - Decisión del orquestador (2026-10-08), validada por el Arquitecto (una consulta; sus ajustes están incorporados). Requiere la revisión de seguridad que esta Enmienda exige.
- **Registro (2026-10-28, clases de trabajo del daemon, RES-06):** `macsys` suma el módulo público `qos` (QoS del hilo actual y prioridad de un hijo antes de `exec`) y el módulo FFI privado `ffi_qos`, con `pthread_set_qos_class_self_np`, `qos_class_self` y `getiopolicy_np`/`setiopolicy_np` declarados a mano (`libc` no los trae) y `setpriority` de `libc`. `lower_child` instala con `CommandExt::pre_exec` un cierre que solo hace `setpriority(PRIO_PROCESS, 0, nice)` y `setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_PROCESS, …)`: dos llamadas al sistema sobre enteros capturados por valor, sin reservar memoria ni tomar cerrojos entre `fork` y `exec`. Sin punteros en la API pública. Sigue las mismas reglas y pasa por revisión de seguridad. Ver ADR-GRP-015 § 1 y DS-TS-GRP-005 (T001, D9).

## Enmienda (2026-10-07, nx sobre cargo y CI con affected)

Decisión del orquestador (2026-10-04), validada por el Arquitecto (`nassa-architect:architect`, una consulta; sus ajustes ya están incorporados). La pide Rene (2026-10-07): que el CI corra solo lo nuevo o lo modificado, y que `nx` y `cargo` compartan la compilación. El detalle y las mediciones están en [DS-INF-GRP-001, Enmienda 2026-10-07](../../requirements/features/motor-local/dev-specs/INF-GRP-001-dev-spec.md#enmienda-2026-10-07-ci-con-affected-y-nx-sobre-el-target-compartido).

- **Se activa el plan B: `nx:run-commands` sobre `cargo`.** `@monodon/rust` 3.0.0 fuerza `RUSTC_WRAPPER=''` en cada cargo que lanza (`src/utils/run-process.js`, sin opción para evitarlo), así que desactivaba `sccache`. Además, los `project.json` le pasaban `--target-dir dist/target/<proyecto>`: varios GB por proyecto, compilados desde cero, y declarados como `outputs`, de modo que la caché de Nx los copiaba. Ahora cada proyecto Rust declara `build` (`cargo build`, o `cargo check` en las librerías), `test`, `lint` (`cargo clippy --all-targets -- -D warnings`) y, en las apps, `run`, todos con `cargo … -p {projectName}`. El plugin se queda **solo para el grafo** (proyectos y dependencias desde `cargo metadata`).
- **Se revierte la mitigación del `target-dir` por target.** Todos los proyectos usan el `target/` de la raíz, el mismo que `cargo test --workspace`, y respetan el `rustc-wrapper` de la configuración de cargo. Los targets llevan `parallelism: false` para no competir por el lock de `target/`, sin `outputs`, y `test` va sin caché de Nx: una dependencia que el grafo no viera repetiría un verde viejo.
- **El grafo declara los binarios que lanzan los tests.** `gitraptor-cli` tiene `implicitDependencies: ["gitraptor-mcp"]` porque sus tests lanzan `raptor-mcp` (`gitraptor_testkit::sibling_bin`) sin depender del crate. Función de fitness: `tools/ci/affected-tests.mjs` falla `lint and test (ubuntu-latest)` si un test lanza el binario de un paquete que su proyecto no alcanza en el grafo.
- **CI: Nx decide qué corre y cargo lo corre en una sola invocación.** En un PR con cambios de Rust, `nx show projects --affected` da los paquetes afectados (y sus dependientes), y los tests generales corren con `cargo test --workspace --exclude <no afectados>`. No se usa `nx affected -t lint test` por proyecto: cargo unifica las features sobre los paquetes seleccionados, y `cargo test -p gitraptor-cli` tras el build del workspace recompila unas 100 unidades (de serde a gix y todos los crates del workspace). La selección suma los paquetes cuyos binarios lanzan los tests afectados (`implicitDependencies`), y si aun así cambiara las features, corre el workspace entero. `fmt` y `clippy` siguen sobre todo el workspace: son baratos con caché, y `clippy` con `--exclude` también recompila. En `push` a `main` siempre se ejecuta todo.
- **D9 de INF-GRP-001 no cambia:** el gate de CI del arnés ejecuta siempre todo el workspace (`cargo test --workspace -- repo_intact`), nunca `nx affected`.

## Enmienda 2026-10-28 (app de escritorio en el MVP, D7)

**Decisión de Rene Bonilla (2026-10-28, D7)**: `apps/desktop` deja de ser Fase 3. La forma de Tauri en el monorepo (frontend como paquete de pnpm y `src-tauri` como miembro del Cargo workspace) no cambia. Lo demás es **Decisión del orquestador (2026-10-28), validada por Arquitecto**. La frontera con el daemon está en [ADR-GRP-017](./ADR-GRP-017-app-escritorio-cliente-daemon.md).

- **Se crea ahora `apps/desktop`**: el paquete de pnpm `@gitraptor/desktop` (Vite + React + TypeScript; scripts `dev`, `build`, `lint`, `typecheck`, `test`, más `tauri dev` y `tauri build`) y `apps/desktop/src-tauri`, el crate `gitraptor-desktop` (`publish = false`, `[lints] workspace = true`, etiquetas Nx `scope:desktop` y `type:app`). `Cargo.toml` añade `apps/desktop/src-tauri` a `members`. `pnpm-workspace.yaml` ya cubre `apps/*`.
- **Dependencias de `gitraptor-desktop`**: solo la biblioteca cliente del canal y, si hace falta, `gitraptor-theme`. Nunca `gitraptor-git` ni `gitraptor-policy`, y de `gitraptor-core` solo el módulo `client` hasta la enmienda E3 de ADR-CKP-003. Lo comprueba V1 de ADR-GRP-017.
- ⚠️ **ASSUMPTION**: las macros de Tauri (`generate_context!`, `generate_handler!`) compilan con `unsafe_code = "forbid"`. Si no es así, la excepción sigue el patrón de `winsys` y `macsys` (enmienda de este ADR, lista `EXCEPTIONS` de `unsafe_boundary.rs` y revisión de seguridad), nunca un `allow` suelto.
- **Paquetes web: todavía no.** `packages/ui-kit`, `packages/graph-renderer` y `packages/core-client` no se crean en UI-1. Su código vive en `apps/desktop/src/{ui,graph,client}` con las mismas reglas: `src/ui` no importa `@tauri-apps/api` ni `src/client` (regla de ESLint dentro de la app, el equivalente de `type:ui`). **Se extraen** a su paquete cuando ocurra lo primero de esto:
  - entra Storybook (UI-2, ADR-GRP-003) → `packages/ui-kit`;
  - aparece un segundo consumidor (la extensión, Fase 3) → `packages/ui-kit`, `graph-renderer` y `core-client`.
- **Tipos del contrato en TypeScript**: se generan desde `crates/api` (ts-rs o specta, a elegir en la Dev Spec) y se commitean en `apps/desktop/src/client/generated`, con una comprobación de "generado al día" en CI, como el workflow `tokens`.
- **`packages/design-tokens`** añade la salida web (variables CSS y tipos TS) a su build de Style Dictionary (ADR-GRP-003, Enmienda 2026-10-28).
- **CI sin cargar al resto**:
  - `src-tauri` arrastra wry, tao y WebKitGTK en Linux. El workspace lo excluye de `default-members`, y los jobs generales (`cargo test --workspace`, `clippy` y el gate `repo_intact` de D9) pasan `--exclude gitraptor-desktop`, para que no exijan las librerías de WebKitGTK ni compilen Tauri en cada historia.
  - Un job nuevo, `desktop`, en `ubuntu-latest` y filtrado por ruta (`apps/desktop/**`, `packages/design-tokens/**`, `crates/api/**`, `crates/core/src/client/**`, `Cargo.lock`, `pnpm-lock.yaml`), corre `pnpm lint`, `typecheck` y `vitest`, más `cargo clippy` y `cargo test -p gitraptor-desktop` con `libwebkit2gtk-4.1-dev`. Siempre reporta el check (en éxito si no aplica), como `repo-intact.yml`.
  - **Sin runners de macOS en PR**: la app de macOS solo se compila en `release.yml` (alphas y releases, ADR-GRP-014).
  - E2E: `tauri-driver` no soporta macOS. En UI-1 se prueba el frontend con el IPC simulado (`mockIPC`) y el backend con tests de Rust. El E2E con WebDriver en Linux queda para UI-2.
- **Pendiente para el orquestador**: actualizar AGENTS.md (Alcance y la regla "`cargo test --workspace` debe pasar", que ahora lleva `--exclude gitraptor-desktop` fuera del job `desktop`).
