---
id: DEV-TS-GRP-001
title: "Dev Spec — Almacén de datos del motor en el perfil"
type: dev-spec
status: approved
feature: motor-local
domain: GRP
story: TS-GRP-001
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-013]
  nfrs: [NFR-01, NFR-03, NFR-11, SEC-02, SEC-06, SEC-07]
tags: [motor-local, perfil, sqlite, rusqlite, directories, clave-de-repo, permisos]
---

# Dev Spec — TS-GRP-001: almacén de datos del motor en el perfil

Blueprint compacto para implementar [TS-GRP-001](../technical-stories/TS-GRP-001-almacen-perfil.md). Fuente de las decisiones: ADR-GRP-006 § 1 a § 4 (con la enmienda del 2026-10-04) y ADR-GRP-013 § 1 y § 4.

## 1. Ubicación en el código

- **Crate**: `crates/core` (`gitraptor-core`), módulo propio `profile`. No toca `crates/git` (TS-GRP-002 va en paralelo): la clave de repo recibe la ruta del **directorio Git común** ya resuelta por quien llama. El daemon (TS-GRP-003) la obtiene de `crates/git`; los tests, de `git rev-parse --git-common-dir` en repos temporales.
- **Submódulos**:

| Archivo | Responsabilidad |
|---|---|
| `profile/mod.rs` | `Profile`: abre o crea el perfil, el índice global y los almacenes por repo. Reexporta la API |
| `profile/dirs.rs` | `ProfileDirs`: resolución por SO, raíz inyectable y sobreescritura por entorno solo en builds de test |
| `profile/fsperm.rs` | Carpetas 0700 y archivos 0600, umask 077, verificación de propietario y modo, aviso de ACL en Windows |
| `profile/repo_key.rs` | Validación de rutas de entrada (SEC-02) y normalización de la ruta del directorio común |
| `profile/sqlite.rs` | Apertura de SQLite (WAL, `synchronous=FULL`), versión de esquema, comprobación de integridad y cuarentena |
| `profile/index.rs` | Índice global: id de instancia y repos observados |
| `profile/store.rs` | Almacén por repo: entidades de ADR-GRP-013 § 1, escritura por lote y consultas |
| `profile/schema.rs` | Migraciones incluidas en el binario (índice y almacén por repo) |
| `profile/error.rs` | `ProfileError` |

## 2. Dependencias nuevas (mínimas)

| Crate | Por qué | Licencia |
|---|---|---|
| `rusqlite` con `bundled` | SQLite embebido, sin depender del SO (ADR-GRP-006 § 4) | MIT (SQLite: dominio público) |
| `directories` | Carpetas estándar por SO (ADR-GRP-006 § 1, PQ-4) | MIT/Apache-2.0 |
| `rustix` (solo Unix, `fs` y `process`) | `geteuid` y `umask` sin `unsafe` (el workspace prohíbe `unsafe_code`) | Apache-2.0/MIT |
| `tempfile` (dev) | Perfiles y repos temporales en los tests | MIT/Apache-2.0 |

Sin `uuid` ni `chrono`: los identificadores opacos salen de `randomblob(16)` de SQLite con formato UUID v4, y las horas las pasa quien llama (`Timestamp` con milisegundos UTC y desfase local), porque el almacén no calcula la zona horaria.

## 3. Ubicación y raíz (`ProfileDirs`)

- `<app>` = **`gitraptor`** (9 caracteres). Con `~/Library/Application Support/gitraptor/state/` la ruta del socket queda muy por debajo de los 104 bytes de macOS (ADR-GRP-005).
- Se construye con `directories::BaseDirs` y se añade `<app>` a mano, para controlar exactamente la tabla de ADR-GRP-006 § 1:

| Carpeta | macOS | Linux | Windows |
|---|---|---|---|
| `data` | `<AppSupport>/gitraptor/data` | `$XDG_DATA_HOME/gitraptor` | `%LOCALAPPDATA%\gitraptor\data` |
| `config` | `<AppSupport>/gitraptor/config` | `$XDG_CONFIG_HOME/gitraptor` | `%LOCALAPPDATA%\gitraptor\config` |
| `state` | `<AppSupport>/gitraptor/state` | `$XDG_STATE_HOME/gitraptor` | `%LOCALAPPDATA%\gitraptor\state` |
| `runtime` | igual que `state` | `$XDG_RUNTIME_DIR/gitraptor` o, si no existe, `state` | `None` (named pipe) |

- Windows usa `data_local_dir()` (local) para las tres carpetas; nunca `config_dir()` ni `data_dir()` (roaming).
- **Raíz inyectable**: `ProfileDirs::under_root(root)` pone `data/`, `config/`, `state/` y `run/` bajo una raíz. Es la vía de los tests y de cualquier llamada desde código.
- **Sobreescritura por entorno**: `ProfileDirs::resolve()` lee `GITRAPTOR_PROFILE_DIR` **solo con `debug_assertions`** (builds de test y desarrollo). En un build release el bloque no se compila y la variable se ignora (SEC-06, H4).
- Carpetas propias que se crean y verifican: la raíz de la app (macOS y Windows), `data`, `data/repos`, `data/quarantine`, `config`, `state` y `runtime`.

## 4. Permisos (SEC-06)

- `Profile::open` fija la umask del proceso a 077 (Unix) antes de crear nada. Además, cada carpeta se crea con modo 0700 y cada archivo con 0600 explícitos, así que no depende de la umask heredada.
- El archivo SQLite se crea vacío con 0600 (`create_new`, sin seguir enlaces) antes de que SQLite lo abra. SQLite crea `-wal` y `-shm` con el mismo modo que el archivo principal.
- **Carpeta preexistente**: se comprueba con `symlink_metadata` que es carpeta (no enlace), que el propietario es el `euid` y que el modo es exactamente 0700. Si no, `ProfileError::InsecureDir` y no se abre; no se "arregla".
- Archivos en cuarentena: se renombran dentro de `data/quarantine/` y se les fuerza 0600.
- Utilidad pública `create_private_file(path)` (0600, `create_new`, `O_NOFOLLOW`) para el lock y los logs de TS-GRP-003.
- **Windows (pendiente explícito, decisión del coordinador del 2026-10-04)**: la comprobación de que la ACL no tiene ACE de otros usuarios necesita FFI Win32 o un crate nuevo, y no se puede probar desde macOS. `verify_windows_acl` no la hace: devuelve el aviso `ProfileWarning::AclNotVerified`, que va en el informe de apertura y se escribe por `stderr`. Requisito antes de cualquier release en Windows.

## 5. Clave de repo (ADR-GRP-006 § 3)

- **Entrada**: la ruta del directorio Git común. Se valida antes de tocar el FS: absoluta y, en Windows, sin UNC, `\\?\`, `\\.\`, dispositivos reservados ni ADS (SEC-02). La validación de la grafía de Windows es una función pura que se prueba en todos los SO y se aplica con `cfg(windows)`.
- **Normalización**: `fs::canonicalize` (resuelve enlaces simbólicos) y, en macOS y Windows, minúsculas sobre la cadena (`key_path`). Se guarda también la ruta canónica tal cual (`canonical_path`) para mostrarla.
- **Clave**: UUID v4 opaco generado al añadir, único por `key_path` en el índice. El commit raíz se guarda como pista opcional (`None` en un repo sin commits).
- **Añadir** devuelve `AddOutcome::New`, `AlreadyObserved` o `Reactivated { retired_at }`. Reactivar conserva la clave y el almacén; quien llama registra el intervalo retirado como hueco (US-GRP-006, fuera de alcance).

## 6. Esquema

Versión de esquema en `PRAGMA user_version`; migraciones en `schema.rs`, aplicadas en una transacción. Una versión mayor que la soportada da `ProfileError::SchemaTooNew` sin escribir en el archivo (comprobación con conexión de solo lectura). Todas las tablas son `STRICT`.

**Índice global** (`data/index.sqlite`, v1):

- `profile_meta(key TEXT PK, value TEXT)`: `instance_id` (UUID generado al crear el perfil; ADR-GRP-006 § 4, enmienda).
- `repos(repo_id TEXT PK, key_path TEXT UNIQUE, canonical_path, state CHECK IN ('observed','retired'), added_at, retired_at NULL, root_commit_hint NULL)`.

**Almacén por repo** (`data/repos/<repo_id>.sqlite`, v1), entidades de ADR-GRP-013 § 1:

- `store_meta(key PK, value)`: `repo_id`, `common_dir`, `observed_until`, `next_seq`.
- `worktrees(id PK, canonical_path UNIQUE, admin_name, first_seen_ms, gone_ms NULL)`.
- `sessions(session_id PK, worktree_id FK, agent_kind CHECK IN ('claude-code','other'), agent_name, initial_origin CHECK IN ('detected','registered'), detection_key, started_ms, ended_ms, end_cause CHECK IN (…))`, índice por worktree.
- `attribution_records(id PK, effective_seq UNIQUE, session_id FK, kind CHECK IN ('register','confirm','correct','withdraw-correction','withdraw-registration'), agent_kind, agent_name, author CHECK IN ('developer','agent'), recorded_ms)`, índice por sesión. **Append-only**: triggers que abortan `UPDATE` y `DELETE`.
- `events(seq PK, worktree_id FK, kind, metadata TEXT, observed_utc_ms, utc_offset_s, session_id NULL, evidence NULL, gap_id NULL)`, índices por sesión y por worktree. **Append-only** con triggers.
- `gaps(gap_id PK, started_ms, ended_ms NULL, cause CHECK IN (…), requested_by NULL)`.
- `last_known_state(worktree_id PK, head, refs, operation, dirty_fingerprint, updated_ms)`.

`seq` es una secuencia monotónica por repo (ADR-GRP-013 § 4) compartida por eventos y registros de atribución (`effective_seq`), que la asigna el almacén dentro de la transacción del lote. El almacén no interpreta `metadata`, `evidence` ni `refs`: son texto que entrega el motor, que solo pone metadatos (NFR-03).

## 7. API pública (resumen)

```rust
pub struct ProfileDirs { data, config, state, runtime: Option<PathBuf>, owned: Vec<PathBuf> }
impl ProfileDirs { fn resolve() -> Result<Self>; fn under_root(root) -> Self; }

pub struct Profile { /* dirs, index */ }
impl Profile {
    fn open(dirs: ProfileDirs) -> Result<(Profile, OpenReport)>;   // crea/verifica, abre índice
    fn instance_id(&self) -> &str;
    fn add_repo(&mut self, common_dir: &Path, root_commit_hint: Option<&str>, now_ms: i64) -> Result<(RepoEntry, AddOutcome)>;
    fn retire_repo(&mut self, repo_id: &str, now_ms: i64) -> Result<()>;
    fn repos(&self) -> Result<Vec<RepoEntry>>;
    fn repo_by_common_dir(&self, common_dir: &Path) -> Result<Option<RepoEntry>>;
    fn open_store(&self, repo_id: &str) -> Result<(RepoStore, StoreOpen)>;   // StoreOpen::Ok | Recovered { quarantined }
}

pub struct RepoStore { /* conexión */ }
impl RepoStore {
    fn write_batch(&mut self, ops: &[WriteOp]) -> Result<BatchResult>;   // una transacción
    fn worktrees / sessions / session / events_for_session / events_for_worktree
       / attribution_records / gaps / last_known_state / observed_until
}

pub enum WriteOp { UpsertWorktree, MarkWorktreeGone, StartSession, EndSession,
                   AppendAttribution, AppendEvent, OpenGap, CloseGap,
                   SetLastKnownState, SetObservedUntil }
```

- **Único escritor** (ADR-GRP-005): las escrituras exigen `&mut self` y el `Profile` es del daemon. El bloqueo de instancia del proceso es de TS-GRP-003.
- **Lote**: `write_batch` abre una transacción `IMMEDIATE`; si una operación falla, se revierte el lote entero y no se consume la secuencia.
- **Integridad al abrir** (índice y almacenes): error de apertura o `PRAGMA quick_check` distinto de `ok` ⇒ se cierra, se mueve el archivo (y `-wal`/`-shm`) a `data/quarantine/<nombre>.corrupt-<unix_ms>` y se crea uno nuevo. El almacén devuelve `StoreOpen::Recovered` para que el motor abra el hueco de "perfil perdido" (Q26). Un repo corrupto no toca los archivos de los demás.
- **SQL parametrizado**: todas las sentencias son literales con `?N`; el helper de consultas solo acepta `&'static str` y las consultas compuestas se arman con `concat!` en compilación. Un test recorre las fuentes del módulo y falla si encuentra `format!` en los archivos con SQL (el "lint de CI" de SEC-06, porque `cargo test` corre en CI).

## 8. Plan de tests

Todos con `tempfile` (perfil y repos temporales); ninguno toca el perfil real ni este repo (NFR-01).

| Criterio de la TS | Test |
|---|---|
| Ubicación por SO; Windows sin roaming | `dirs::tests::layout_matches_adr_table_{macos,linux,windows}` (función pura con las carpetas base de cada SO), `real_layout_resolves_without_creating_anything`, `windows_uses_local_appdata_only` (con `cfg(windows)`) |
| Override solo en builds de test | `tests/profile_override.rs::profile_override_only_in_debug_builds` (fija la variable en un proceso hijo; con `cargo test --release` comprueba que se ignora) |
| Permisos 0700/0600 incl. `-wal`/`-shm` | `tests/profile_permissions.rs::dirs_and_sqlite_files_are_private` |
| Carpeta 0755 o enlace impide abrir | `profile_permissions::preexisting_0755_dir_is_rejected_and_left_alone`, `symlinked_profile_dir_is_rejected`, `quarantined_files_are_private` (otro propietario no se puede simular sin root; lo cubre la misma comprobación de `uid`) |
| ACL en Windows | **No verificable**: aviso `AclNotVerified` (pendiente) |
| SQL parametrizado | `profile::tests::sql_is_never_built_with_format` |
| Clave: worktrees, clones, repo vacío, mayúsculas | `tests/profile_repo_key.rs` |
| Retirar y volver a añadir | `profile_repo_key::retire_and_readd_recovers_key_and_data` |
| Corrupción aislada | `tests/profile_store.rs::corrupt_store_is_quarantined_others_untouched`, `damaged_pages_fail_the_integrity_check`, `corrupt_index_is_quarantined` |
| Esquema más nuevo | `profile_store::newer_schema_is_rejected_and_file_untouched` |
| Persistencia tras `kill -9` | `tests/profile_persistence.rs` (re-ejecuta el binario de test como hijo, lo mata y reabre) |
| Privacidad (NFR-03) | `profile_store::store_never_contains_file_content` |
| Lote atómico, append-only, secuencia | `profile_store::batch_round_trips_every_entity`, `batch_is_atomic`, `events_and_records_are_append_only`, `seq_is_monotonic_across_reopen` |
| Id de instancia | `profile_store::instance_id_stable_across_reopen_and_new_on_recreate` |
| Validación de rutas Windows (SEC-02) | `repo_key::tests::windows_path_validation` (función pura) |

**Fuera del plan** (con su dueño): `cargo-deny`/`cargo-audit` en CI (SEC-07; aún no hay pipeline de CI en el repo), arnés de repo intacto (INF-GRP-001), verificación manual en Linux y Windows.

## 9. Fuera de alcance (y a qué TS pertenece)

- Reglas de resolución de la atribución (US-GRP-009, US-GRP-010), reconciliación y huecos (US-GRP-005, US-GRP-006), lectura de la configuración (US-GRP-013), retención, copia de seguridad y reenlace de repos movidos (TS-GRP-001, fuera de alcance).
- Registro de auditoría de comandos reservados en el índice global (ADR-GRP-013 § 1): TS-GRP-004.
- Tablas de Guardrails (`guardrails_decisions`, diario de instalación, rama base y suelo confirmados) y spool/instantánea del modo degradado: TS de Guardrails (ADR-GRD-001, 003, 004, 006).
- Lock de instancia, logs y socket: TS-GRP-003 y TS-GRP-004 (usan `create_private_file` y las carpetas `state`/`runtime`).
