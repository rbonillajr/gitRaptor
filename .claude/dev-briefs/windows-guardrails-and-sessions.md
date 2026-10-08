# Brief — Guardrails y sesiones de Claude Code en Windows

Amplía DS-US-GRD-001 (Enmienda 2026-10-08) y DS-US-GRP-007 (Enmienda 2026-10-08). Sin ids nuevos.

## Hallazgos del survey (lo que ya existe)
- Dispatcher nativo `raptor-hook.exe` ya existe (`apps/cli/src/bin/raptor-hook.rs`, `STUB_FILE` con `.exe`). El spike § 14 midió que Git for Windows ejecuta un PE nativo como hook (≈6 ms vs 43 ms de `sh`): **XP-32 queda decidido por la medida: nativo, sin `sh`**.
- El canal de Windows ya existe (#158) y `ctx.dirs.runtime` es `Some` en Windows: `InstallBlocker::PlatformUnsupported` ya no se dispara por plataforma; solo queda el corte `cfg!(windows)` en `apps/cli/src/guard.rs` (install/status/uninstall/cancel/log).
- `crates/git/src/guard_write/portable.rs` escribe la carpeta sin DACL (comentario "pendiente con el canal de Windows") y sin FileId.
- Segunda línea: en Windows `SystemProcs.args()` es `None` y `evaluates()` evalúa siempre (fail-closed, test `on_windows_the_real_command_line_is_unreadable_and_evaluated`). No hay que portar nada; hay que verificarlo en máquina real.
- Coexistencia (#193): `prior.rs` ya considera `.exe`; se verifica en máquina real con un hook previo `.exe` y otro sin extensión.
- Detección: `detect/procs.rs` solo tiene macOS (libproc) y Linux (/proc); `detection_supported()` es `false` en Windows. `winsys::process` ya da pid, ppid, creación (100 ns), exe y owner; **falta el cwd** de otro proceso.

## Partición propuesta: 2 PR (disjuntos)
- **PR A (esta rama `feat/windows-guardrails-and-sessions`) — Guardrails**
- **PR B (rama `feat/windows-session-detection`, desde `main` actualizado) — Sesiones**

## File & Project Topology
### Slice A — Guardrails
- `apps/cli/src/guard.rs` — quitar los 5 `if cfg!(windows)` y la clave i18n `guard.unsupported-platform` si queda sin uso.
- `crates/git/src/guard_write/portable.rs` — crear la carpeta temporal con `winsys::acl::create_private_dir` (DACL protegida: usuario, SYSTEM, Administrators; heredada por dispatchers, `dispatch.conf` y manifiesto); `FileId` real por `winsys::file_id` si existe el helper.
- `crates/git/Cargo.toml` — dependencia `gitraptor-winsys` solo `cfg(windows)` (solo si hace falta).
- `crates/core/src/guardrails/install.rs` — tras escribir, verificar con `acl::verify_private_dir`; si falla, rollback + blocker.
- tests: `crates/git` (DACL de la carpeta creada, `#[cfg(windows)]`), `apps/cli/tests/` (guard install/status/uninstall en Windows con repo temporal), docs: enmienda DS-US-GRD-001, XP-32 y release-plan.

### Slice B — Sesiones
- `crates/winsys/src/process.rs` + `ffi_process.rs` — `cwd(pid, created_100ns)`: `NtQueryInformationProcess(ProcessBasicInformation)` → PEB → `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory` con `ReadProcessMemory` **solo de ese campo** (nunca `CommandLine` ni `Environment`, SEC-04); abre con `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`, verifica el par (pid, creación) antes y después; todo `unsafe` queda en `ffi_process.rs`.
- `crates/core/src/detect/procs.rs` — `SystemProcLister` en Windows (lista por `winsys::process::pids`, solo procesos del usuario actual, `start_us` desde la hora de creación), cwd sin prefijo verbatim; `detection_supported()` incluye Windows.
- `is_claude` (`detect/mod.rs`): reconocer `claude.exe` por ruta del ejecutable (nombre sin distinguir mayúsculas y extensión `.exe`).
- tests: `detect/tests.rs` (tabla sintética con rutas Windows), `winsys` (cwd de un hijo propio, proceso ajeno denegado, pid reutilizado), `apps/cli/tests/claude_sessions.rs` en Windows.
- docs: enmienda DS-US-GRP-007 y ADR-GRP-012 (fila Windows), XP-35.

## Convenciones observadas
- Todo `unsafe`/FFI en `crates/winsys/src/ffi_*.rs`, API segura en el módulo hermano (`ffi_acl.rs`/`acl.rs`).
- `cfg(windows)` en tests con `#[cfg(windows)]` y repos temporales (`second_line.rs::on_windows_the_real_command_line_is_unreadable_and_evaluated`).
- Mensajes de usuario por i18n `t("guard.…")` (en/es).

## Decisiones (Decisión del orquestador, 2026-10-08)
1. Dispatcher nativo en Windows, sin `sh` (medida del spike § 14; ADR-GRD-001 Enmienda 2026-10-05 ya lo adopta en los tres SO). Ya validado: no requiere consultar.
2. DACL por `create_private_dir` en la carpeta temporal previa al `rename` (la DACL protegida de la carpeta se hereda por sus archivos); tras el `rename` el SD se conserva.
3. cwd por PEB, solo el campo `CurrentDirectory`: la alternativa (sin cwd) invalidaría S1/S3 (la sesión se define por cwd en el worktree). Decisión nueva de diseño → se valida con Arquitecto.
4. Intérprete (`node …/cli.js`, instalación npm) sigue sin detectarse, igual que en macOS/Linux; no hay falsos positivos.

## Not Built (deferred)
Windows 11 / Windows Terminal (XP-34), `argv[1]` de intérpretes, atribución S3 en Windows más allá de lo que da el cwd, validación Linux.

## Mejoras detectadas (no se hacen)
Ninguna fuera de alcance.

## Contrato (criterios → verify)
- G1 `cargo test -p gitraptor-git guard_write` — la carpeta creada tiene DACL privada (Windows real).
- G2 `cargo test -p gitraptor-cli --test guard_windows` (nuevo): install protege, force-push y borrado de la rama base bloqueados, uninstall restaura.
- G3 `cargo test -p gitraptor-core second_line` — Windows siempre evalúa.
- S1 `cargo test -p gitraptor-winsys cwd` — cwd propio/ajeno/pid reutilizado.
- S2 `cargo test -p gitraptor-core detect` — `claude.exe` detectado con tabla sintética.
- S3 `cargo test -p gitraptor-cli --test claude_sessions` en Windows: `raptor status` muestra la sesión.
- Aceptación manual en la máquina real con binario release (conteo passed/failed antes y después en el PR).
