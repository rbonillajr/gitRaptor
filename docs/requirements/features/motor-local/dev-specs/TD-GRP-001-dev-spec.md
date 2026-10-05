---
id: DS-TD-GRP-001
title: "Dev Spec — Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06)"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
created: 2026-10-05
updated: 2026-10-05
related:
  stories: [TD-GRP-001, TS-GRP-001, TS-GRP-002]
  adrs: [ADR-GRP-002, ADR-GRP-006, ADR-GRP-009]
  nfrs: [NFR-01, NFR-11, SEC-06, SEC-10]
tags: [motor-local, windows, acl, dacl, sid, seguridad, unsafe, windows-sys, winsys, perfil, resolucion-git]
---

# Dev Spec — TD-GRP-001: Verificación de ACL en Windows

Plano compacto (AADD ligero) de [TD-GRP-001](../technical-stories/TD-GRP-001-acl-windows.md). Cierra los dos pendientes de Windows: las ACE del `git.exe` de [TS-GRP-002](./TS-GRP-002-dev-spec.md) (hoy todo se rechaza con `AclUnverified`, *fail-closed* del 2026-10-04) y la ACL del perfil de [TS-GRP-001](./TS-GRP-001-dev-spec.md) (hoy, aviso `AclNotVerified`). Son la causa A de unos 65 fallos de la primera pasada de tests en la Windows real.

## 1. Decisiones

| # | Decisión | Por qué |
|---|---|---|
| D1 | **Las llamadas Win32 van en `crates/winsys` (`gitraptor-winsys`)**, el único crate con `unsafe` permitido, compartido con la resolución del solicitante en Windows (rama `feat/windows-requester-resolution`; la excepción se registra en la enmienda de ADR-GRP-002 de esa rama). El crate declara `#![deny(unsafe_code)]`; el `unsafe` vive solo en módulos FFI privados con `#[allow(unsafe_code)]` (`ffi_acl` para esta TD) y cada bloque lleva `// SAFETY:` (`clippy::undocumented_unsafe_blocks = "deny"`, `multiple_unsafe_ops_per_block = "deny"`). El módulo público `acl` es seguro y no expone punteros ni handles. `windows-sys = "0.61"` se declara en `workspace.dependencies` (sin `=`, para que se actualice junto con `gix`, que ya lo trae a `Cargo.lock`). `crates/git` y `crates/core` dependen de `winsys` solo con `cfg(windows)` | `forbid` del workspace impide un `allow` local dentro de un crate, así que el aislamiento tiene que ser un crate. `windows-sys`: MIT OR Apache-2.0 (NFR-11), de Microsoft, sin dependencias nuevas en el lock. Se descartan `windows-permissions` (sin release desde 2021), `windows-acl` (2019, sobre `winapi`) y `windows` (las llamadas Win32 siguen siendo `unsafe`, y pesa más). Ejecutar `icacls` o PowerShell también se descarta: su salida está localizada y sería un programa externo |
| D2 | **Lectura por handle**: `CreateFileW(READ_CONTROL, FILE_SHARE_READ \| WRITE \| DELETE, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS \| FILE_FLAG_OPEN_REPARSE_POINT)` y luego `GetSecurityInfo(OWNER \| DACL)`. El SID del usuario se lee con `GetTokenInformation(TokenUser)` del token del proceso Además se leen los atributos (`GetFileInformationByHandle`) y las marcas del volumen (`GetVolumeInformationByHandleW`): un componente que es *reparse point* da `ReparsePoint` y un volumen sin `FILE_PERSISTENT_ACLS` (FAT, recurso de red), `NotLocal`. Solo se aceptan rutas de disco local (`C:\`, `\\?\C:\`); una ruta UNC da `NotLocal`. La DACL se copia a `Vec<u8>` y se analiza en Rust seguro, validando `AclSize`, `AceSize` y la longitud de cada SID | Con un handle, la ruta no puede cambiar entre abrirla y leerla. Con `OPEN_REPARSE_POINT` se leería la ACL del enlace y no la de su destino, así que un enlace en la cadena se rechaza; la ruta que se ejecuta es la canónica, que no pasa por ninguno. En un recurso de red, "Administrators" serían los del servidor (Arquitecto; security-expert M-02, M-03) |
| D3 | **SID de confianza**: el usuario actual, `SYSTEM` (S-1-5-18), `Administrators` (S-1-5-32-544) y `TrustedInstaller` (S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464). Se comparan con `EqualSid` contra SID construidos con `CreateWellKnownSid` o desde la autoridad, nunca por nombre | Son los equivalentes de "el usuario o root" en Unix. Un nombre está localizado y se puede suplantar; un SID no |
| D4 | **Regla de DACL** (*fail-closed*): si la DACL falta o es NULL, se rechaza. Se mira cada ACE que se aplica al propio objeto, es decir, sin `INHERIT_ONLY_ACE`. Se rechaza un `ACCESS_ALLOWED_ACE_TYPE` con algún bit peligroso para un SID que no es de confianza, y también cualquier otro tipo de ACE de permitir (callback, objeto o compuesto). Los ACE de denegar se ignoran: solo quitan acceso, así que ignorarlos puede rechazar de más, pero nunca aceptar de más. Si la ACL no se puede leer, se rechaza | Lo que no se entiende se rechaza |
| D5 | **Bits peligrosos para `git.exe`** (SEC-10), con el propietario de cada objeto de confianza (D3). **En el archivo**: `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_WRITE`, `GENERIC_ALL` y `MAXIMUM_ALLOWED`. **En su carpeta**: `FILE_ADD_FILE`, `FILE_ADD_SUBDIRECTORY`, `FILE_WRITE_EA`, `FILE_DELETE_CHILD`, `FILE_WRITE_ATTRIBUTES`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_WRITE` `GENERIC_ALL` y `MAXIMUM_ALLOWED`; no se permite añadir archivos para evitar el *DLL planting* junto al ejecutable. **En cada carpeta superior, hasta la raíz del volumen**: `FILE_DELETE_CHILD`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_WRITE`, `GENERIC_ALL` y `MAXIMUM_ALLOWED`, porque quien puede renombrar o borrar una carpeta superior puede sustituir la ruta | Medido en la Windows real: `C:\` da a *Authenticated Users* `FILE_ADD_SUBDIRECTORY` (AD) sobre la raíz. Crear carpetas nuevas no permite sustituir `C:\Program Files`, así que en las carpetas superiores ese bit no es peligroso. `OWNER RIGHTS` (S-1-3-4) cuenta como de confianza porque el propietario ya lo es (security-expert L-01) |
| D6 | **`git.exe`**: en Windows, `check_owner_and_mode` llama a `winsys::acl::verify_trusted_executable`. Un propietario no fiable da `UntrustedOwner`; un ACE de escritura de otro SID, `WritableByOthers`; una ACL ilegible o un tipo de ACE desconocido, `AclUnverified`. Ninguna ubicación se acepta por estar en `%ProgramFiles%`: se verifica igual que cualquier otra. Si el candidato es el lanzador de Git for Windows (`<raíz>\cmd\git.exe`), también se verifican el Git que lanza y su carpeta cuando existen: `<raíz>\{mingw64,ucrt64,clangarm64,mingw32}\bin\git.exe` y `<raíz>\bin\git.exe` (en la Windows real, Git 2.56 usa `ucrt64`) | Mismo vocabulario de rechazo que en Unix. `AclUnverified` queda para lo que no se pudo comprobar. El lanzador ejecuta otro binario, que también se verifica, junto con la carpeta de sus DLL (security-expert M-01) |
| D7 | **Perfil (SEC-06)**: en Windows, `verify_private_dir` exige un propietario que sea el usuario, SYSTEM o Administrators (sin TrustedInstaller). Además, **todo** ACE de permitir debe ser de uno de esos tres SID (o de `OWNER RIGHTS`), para cualquier acceso, también la lectura, **incluidos los ACE solo heredables** (`INHERIT_ONLY`), que los archivos nuevos heredarían. Si no se cumple, da `ProfileError::InsecureDir` y el motor no arranca; la carpeta nunca se "arregla", igual que en Unix. Cada **componente nuevo** de una carpeta del perfil se crea con `CreateDirectoryW` y un descriptor con el usuario como propietario y una DACL **protegida** (sin herencia) que da control total `(OI)(CI)` al usuario, SYSTEM y Administrators (`O:<usuario>D:P(A;OICI;FA;;;<usuario>)(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)`). Si el componente ya existe (`ERROR_ALREADY_EXISTS`), no se da por bueno: se verifica como cualquier carpeta existente. Los archivos heredan esa DACL. Se eliminan `ProfileWarning` y `OpenReport::warnings`, que solo existían para ese aviso | Crear la carpeta con el descriptor desde el principio evita una ventana con permisos abiertos, como la umask 077 en Unix. Un ACE solo heredable de otro usuario llegaría a la base SQLite y a los logs (security-expert H-01). Como propietario se aceptan también SYSTEM y Administrators, a diferencia de Unix: ya pueden tomar la propiedad de cualquier objeto, así que excluirlos no protege nada, y un proceso elevado crea por defecto objetos de Administrators (Arquitecto, opción b). Las carpetas superiores del perfil (`%LOCALAPPDATA%`) son del usuario y no se comprueban, igual que en Unix |
| D8 | **Tests en la Windows real** (`#[cfg(windows)]`, con carpetas temporales, nunca el perfil real). Se aceptan el `git.exe` de `C:\Program Files\Git\cmd`, si existe, y un archivo en una carpeta temporal recién creada. Se rechazan: un `git.exe` con ACE de escritura para `Users`, una carpeta con `FILE_ADD_FILE` para `Users`, una carpeta superior con `FILE_DELETE_CHILD` para `Everyone` y un propietario `Users`. En el perfil, una carpeta con lectura para `Users` se rechaza sin que cambie su ACL, y una carpeta creada lleva la DACL protegida. Además: un *junction* en la cadena de `git.exe` se rechaza con `ReparsePoint`, y la ruta canónica que lo resuelve se acepta; una carpeta del perfil con un ACE `(OI)(IO)` para `Users` se rechaza. El análisis de la DACL y las reglas no dependen del SO y tienen tests unitarios que también corren en macOS (ACL del `git.exe` y de `C:\` medidas en la máquina, ACL truncadas o con tamaños falsos). Los tests ponen los ACE hostiles con `icacls` | Son los casos negativos que pide la tarea. `icacls` solo se usa en los tests |

## 2. Interfaz de `winsys::acl` (solo Windows)

```rust
pub struct Sid(/* opaco */);                                          // PartialEq vía EqualSid
pub fn current_user_sid() -> io::Result<Sid>;
pub enum AclError { Unreadable(io::Error), NotLocal, ReparsePoint, NullDacl, Malformed, UnknownAce(u8),
                   UntrustedOwner(Sid), UntrustedWriter(Sid), UntrustedAccess(Sid) }
pub fn parse_acl(bytes: &[u8]) -> Result<Vec<Ace>, AclError>;           // todos los SO
pub fn evaluate(s: &Security, role: Role, user: &Sid) -> Result<(), AclError>; // todos los SO
pub fn verify_trusted_executable(path: &Path) -> Result<(), AclError>; // D4–D6: archivo y carpetas superiores
pub fn verify_private_dir(path: &Path) -> Result<(), AclError>;        // D7
pub fn create_private_dir(path: &Path) -> io::Result<()>;              // D7: un componente
```

## 3. Validación

Todas las decisiones son **Decisión del orquestador (2026-10-05), validada por Arquitecto y security-expert**.

- **Arquitecto**: aprobada con ajustes, todos incorporados. Son los ACE solo heredables en el perfil, el usuario como propietario al crear, el rechazo de *reparse points*, el nombre de `FILE_DELETE_CHILD`, los dos tests negativos más y `windows-sys = "0.61"` en el workspace. También pidió las enmiendas de ADR-GRP-009 § 4 y ADR-GRP-006 § 1.
- **security-expert**: gate aprobado con condición. El único alto (H-01, ACE solo heredables en el perfil) está corregido. M-01 a M-03 y L-01 están incorporados. L-02 (comparar con `GetFinalPathNameByHandleW`) no hace falta: la ruta ejecutada es la canónica verificada y ningún componente puede ser *reparse point*. L-03 (archivos con un ACE explícito propio) queda fuera: los crea el daemon y heredan la DACL protegida.

## 4. Criterios de aceptación observables

1. En la Windows real, `resolve` acepta `C:\Program Files\Git\cmd\git.exe` y rechaza el caso hostil (escritura para `Users`) con `WritableByOthers`.
2. Los tests de la causa A dejan de fallar por `AclUnverified` o `AclNotVerified`. El PR lleva el conteo de passed/failed antes y después.
3. `cargo clippy --all-targets -- -D warnings` y `cargo test` pasan en macOS y en el CI (`windows-latest`).
4. La revisión de `nassa-security:security-expert` no deja hallazgos críticos ni altos abiertos.

## 5. Fuera de alcance

- Verificar el árbol completo de la instalación de Git, más allá del lanzador, el Git que lanza y la carpeta de sus DLL. Los shims de Scoop viven en la carpeta del usuario y son de su confianza, como `~/.nix-profile` en Unix (riesgo residual registrado en ADR-GRP-009).
- La ruta de instalación leída del registro (ADR-GRP-009 § 4) sigue pendiente.
- Usuarios no administradores en la Windows real: la máquina de pruebas solo tiene una cuenta de administrador.
