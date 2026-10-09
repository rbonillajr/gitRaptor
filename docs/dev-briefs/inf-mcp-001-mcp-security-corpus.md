---
title: "Brief de implementación — corpus de seguridad del MCP como suite de CI (INF-MCP-001)"
type: dev-brief
status: draft
created: 2026-10-09
updated: 2026-10-09
feature: mcp
stories: [INF-MCP-001]
related:
  adrs: [ADR-MCP-001, ADR-GRP-009, ADR-GRP-002, ADR-GRP-016]
  rules: [BR-16, NFR-01, NFR-02, SEC-MCP-05, SEC-MCP-06, SEC-MCP-08, SEC-MCP-09, SEC-MCP-11]
  specs: [DS-US-MCP-003, DS-US-MCP-004, DS-US-MCP-005, DS-US-MCP-008]
contract: docs/dev-briefs/inf-mcp-001-mcp-security-corpus.contract.json
---

# Brief: corpus de seguridad del MCP (INF-MCP-001)

Autor: `rust-architect`. Ejecuta: `rust-expert`. Rama: `feat/INF-MCP-001-mcp-security-corpus`.
Dev Spec de la historia: `DS-INF-MCP-001` (la escribe el slice E; el id lo da `devspec-target`, que lo toma de la historia y no reserva número).

## 1. Contexto y objetivo

`raptor-mcp` ya sirve `status` y `snapshot` (US-MCP-003, 004, 005 y 008). Sus rechazos se prueban hoy en tests sueltos: `apps/mcp/tests/handshake.rs` y los E2E de `apps/cli/tests/mcp_allowlist.rs` y `mcp_snapshot.rs`, que solo corren en macOS. Esta historia añade **un corpus declarativo de ataques** (un JSON por caso, y un caso nuevo no exige código), **un arnés** que lo ejecuta contra los binarios reales y aplica las mismas comprobaciones a todos los casos, y **un gate de CI** que imprime el KPI de Q-MCP-18 ("% del corpus rechazado") y falla por debajo del 100 %. Los casos de herramientas futuras quedan como filas pendientes en la historia, y la historia termina en `partially-implemented`.

## 2. Qué existe y qué falta

- **Existe**: los binarios `raptor` (daemon) y `raptor-mcp`; la máquina temporal (`Fixture`) y la huella del repo (`fingerprint`, `Exceptions`) de INF-GRP-001; `sibling_bin`; el patrón de agente simulado y de comandos reservados por pty; y los códigos estables de `McpToolError`.
- **Falta**: el modelo del caso y su cargador, el juez (comprobaciones comunes), el escáner de secretos, el informe con el KPI, el runner con dos niveles (sin motor y con motor), los ~60 archivos de caso, el workflow `mcp-security-corpus.yml` y la documentación.
- **No cambia código de producción**: ni `apps/mcp/src`, ni `crates/api`, ni `crates/core`. Si un caso destapa un fallo del servidor, se reporta como **BLOQUEO** (ver § 8.4). No se arregla en esta rama.

### Convenciones observadas

- `apps/mcp/tests/handshake.rs:session`: `raptor-mcp` se lanza con `env_clear()` y `GITRAPTOR_PROFILE_DIR` hacia una carpeta temporal. stdout solo lleva JSON-RPC y stderr va vacío en una sesión sana.
- `apps/mcp/tests/handshake.rs:undeclared_parameters_are_malformed`: un parámetro no declarado devuelve el error `-32602` con `message: "invalid-params"` y `data.field.untrusted`. Una herramienta desconocida devuelve `-32602` con `message: "unknown-tool"`. Ninguno de los dos crea el perfil, así que el motor nunca se toca.
- `apps/mcp/src/server.rs:malformed`: el nombre del campo de `invalid-params` pasa por `UntrustedName::mcp_name` y `for_mcp`, así que sale escapado y cortado.
- `apps/mcp/src/server.rs:respond`: un rechazo es `isError: true` con un solo bloque de texto `{code, message, action[, params]}` y **sin** `structuredContent`. Un éxito lleva la misma serialización en texto y en `structuredContent`.
- `apps/mcp/src/server.rs:status_tool` y `apps/mcp/src/snapshot.rs:tool`: el `outputSchema` de cada herramienta sale de `schema_for!` más `compact_schema`. Es la allowlist de campos de los éxitos.
- `apps/mcp/src/server.rs:tests::conforms`: hay un comprobador privado del subconjunto de JSON Schema que dejan los esquemas compactos. Solo existe en los tests del binario.
- `apps/mcp/src/status.rs:cursor_argument` y `crates/api/src/methods/mcp.rs:valid_cursor`: el cursor son exactamente 16 caracteres `[0-9a-f]`. Cualquier otra cosa es `invalid-params` con `field: "cursor"`, antes del motor.
- `apps/mcp/src/snapshot.rs:label_argument` y `apps/mcp/src/snapshot.rs:check_label`: un argumento desconocido o una etiqueta que falta o no es string da `invalid-params`. Una etiqueta inválida (`crates/api/src/catalog.rs:check_snapshot_label`) da el rechazo de dominio `invalid-text` con `params {field, max_chars}`, **antes** del motor.
- `apps/mcp/src/engine.rs:refusal`: `SCOPE_REFUSED` se traduce a `repo-not-enabled` o `not-in-observed-worktree`, y un cursor desconocido para el daemon, a `invalid-cursor`.
- `apps/mcp/src/main.rs:main`: stderr solo lleva líneas `raptor-mcp: <código>`, y el panic hook imprime `raptor-mcp: internal-error`.
- `crates/api/src/mcp_view.rs:MAX_MCP_PART_BYTES`, `MCP_REFUSAL_TOKENS`, `MCP_BYTES_PER_TOKEN` y `MAX_MCP_NAME_CHARS` son los topes de ADR-MCP-001 § 6 y de RES-MCP-03. `crates/api/src/mcp_view.rs:McpToolError::as_str` da los códigos estables.
- `apps/cli/tests/mcp_allowlist.rs:Machine`: el daemon corre con `GITRAPTOR_PROFILE_DIR`, `GITRAPTOR_AGENT_EXECUTABLES=raptor-fake-agent` y `PATH=/usr/bin:/bin`. `Machine::developer` ejecuta los comandos reservados bajo pty con `/usr/bin/script`. `Drop` mata el daemon con `running_pid`.
- `apps/cli/tests/mcp_allowlist.rs:fake_agent_entry`: el agente simulado es una copia del binario de test llamada `raptor-fake-agent`, que lanza el argv de `RAPTOR_FAKE_AGENT_ARGV`.
- `apps/cli/tests/mcp_allowlist.rs:assert_refused`: los rechazos de ámbito tienen exactamente las claves `action`, `code` y `message`. `apps/cli/tests/mcp_allowlist.rs:is_hidden` define los caracteres ocultos de L-03.
- `apps/cli/tests/mcp_snapshot.rs:Mcp::read` y `apps/cli/tests/mcp_snapshot.rs:agent_command`: ninguna lectura de la respuesta del servidor espera sin un plazo, y el `raptor-mcp` del agente hereda las tuberías del agente.
- `apps/cli/tests/daemon_process.rs:in_pty`: en Linux, la pty usa la sintaxis de util-linux, `script -qec "<argv>" /dev/null`.
- `apps/cli/tests/guard_machine/mod.rs:Machine::developer`: un módulo de soporte vive en una subcarpeta de `tests/` que Cargo no descubre como target.
- `crates/testkit/src/fixture.rs:Fixture::with_commit`, `Fixture::add_worktree` y `Fixture::snapshot`: la máquina temporal y su huella por scope de primer nivel. Los worktrees viven en `<root>/wt-<name>`.
- `crates/testkit/src/exceptions.rs:Exceptions::engine_profile` y `Exceptions::filter`, con `crates/testkit/src/fingerprint.rs:diff`: la única diferencia admitida es la de los datos del motor en el perfil.
- `crates/testkit/src/canary.rs:script`: los scripts trampa de Unix se escriben con `/bin/sh` para evitar `ETXTBSY`.
- `crates/testkit/src/sibling.rs:sibling_bin`: localiza `raptor-mcp` junto a `CARGO_BIN_EXE_raptor`. No recompila uno que esté viejo.
- `crates/testkit/src/lib.rs`: la cabecera del módulo enumera cada submódulo, y uno nuevo se añade a esa lista. El testkit no depende de ningún crate de GitRaptor.
- `crates/testkit/tests/dev_only.rs:testkit_is_only_a_dev_dependency`: el testkit solo puede ser una dev-dependency.
- `tools/ci/affected-tests.mjs:missingEdges`: un test que lanza otro binario con `sibling_bin` necesita la arista en Nx. `apps/cli/project.json` ya declara `implicitDependencies: ["gitraptor-mcp"]`.
- `.github/workflows/repo-intact.yml`: las acciones van fijadas por SHA, rust-cache solo guarda en `main` y nunca se tocan variables `RUST_*` ni `CARGO_*`. El paso de tests generales (`--skip repo_intact`) ya corre en los tres SO todo test de `apps/cli`.
- `.config/nextest.toml`: nextest mata un test a los 240 s.
- `rmcp-3.5.0/src/transport/async_rw.rs:receive`: una línea que no es JSON se ignora sin respuesta. Un JSON con forma inválida recibe `-32600 "Invalid request"` sin `id`. El códec de `stdio()` no limita la longitud de la línea.

### Mejoras detectadas

- **`raptor-mcp` no aplica el tope de entrada de ADR-MCP-001 § 6** (≤ 1 MiB y profundidad ≤ 32): `stdio()` usa el códec sin `max_length` (`JsonRpcMessageCodec::new_with_max_length` existe) y serde corta en 128 niveles. El corpus lo **documenta** con dos casos que hoy sí se rechazan por otra vía (`invalid-params` o una línea ignorada), y deja la fila pendiente "mensaje > 1 MiB o > 32 niveles". Lo corrige una historia de `apps/mcp` (dueña natural: seguimiento de US-MCP-005), no esta rama.
- **El comprobador `conforms` está duplicado**: nace en `crates/testkit` (slice A). Los tests de `apps/mcp/src/server.rs` y `status_tests.rs` pueden pasar a usarlo cuando `apps/mcp` tome `gitraptor-testkit` como dev-dependency. Fuera de esta rama.
- **`mcp_allowlist.rs` y `mcp_snapshot.rs` repiten la `Machine`**: el runner del corpus define una tercera. Se pueden unificar sobre `apps/cli/tests/mcp_corpus/` cuando se toquen esos E2E.
- **La historia, el ADR (§ 9, fila MCP01) y SEC-MCP-08 nombran "gitleaks"**: el corpus usa un escáner propio (D7). Hay que ajustar el texto, pero es una decisión del coordinador (§ 12, P1).
- **El corpus corre dos veces en Ubuntu**: en `lint and test (ubuntu-latest)` y en el gate nuevo. Se podrá excluir del primero si el gate nuevo pasa a ser obligatorio.

## 3. Decisiones de arquitectura

| # | Decisión | Por qué |
|---|---|---|
| D1 | **Lógica pura en `crates/testkit/src/mcp_corpus/`** (modelo del caso, juez, escáner, esquema, informe) y **runner en `apps/cli/tests/mcp_corpus/`** | El juez se prueba sin binarios, en milisegundos y en los tres SO, con fallos inyectados: es donde viven las autopruebas de mutación. El runner necesita `CARGO_BIN_EXE_raptor`, `gitraptor_api`, `gitraptor_core` y el agente simulado, y `apps/cli` ya tiene todo eso más la arista Nx a `gitraptor-mcp`. El testkit no puede depender de crates de GitRaptor, así que el runner le pasa los topes (`Limits`) |
| D2 | **Un test, un informe**: `the_corpus_is_fully_rejected` ejecuta todos los casos en un pool de ≤ 4 hilos y calcula un único KPI | El KPI es una razón sobre todo el corpus. Con un test por caso, nextest lo partiría en procesos y no habría informe |
| D3 | **Casos en JSON** (`serde_json`), un archivo por caso, con **esquema cerrado**: una clave desconocida es un error de carga | `serde_json` ya está en el testkit y en `apps/cli`, y TOML añadiría un crate. Un esquema cerrado hace que una errata falle en voz alta en vez de desactivar un caso en silencio |
| D4 | **Dos niveles**: `server` (rechazado antes del motor; perfil intacto con cero excepciones; todos los SO) y `engine` (daemon real en perfil temporal; macOS y Linux) | Lo que se rechaza por esquema no necesita daemon y tiene que correr en Ubuntu y Windows. El ámbito, la allowlist y el rate limit los decide el daemon |
| D5 | **Estado declarativo del daemon**: `repo` y `other_repo` en `none`, `observed` o `enabled`, más `worktrees`, `symlinks`, `dirs` y `path_trap`. El runner lo materializa con `raptor repo add` y `raptor mcp enable` bajo pty | Es lo que hace el desarrollador real, por los comandos reservados. No se escribe el perfil a mano, que acoplaría el arnés al formato del almacén |
| D6 | **Regla de veredicto única**: la primera respuesta que no es un éxito debe casar con `expect`. Si todas son éxitos, `NotRejected` | Vale igual para una llamada y para `repeat` (el caso del rate limit). Los éxitos previos se comprueban igual contra la allowlist y los topes |
| D7 | **Escáner de secretos propio**, sin gitleaks: ocho canarios exactos plantados en el repo, la config, el commit, el `.env` y el entorno, más un detector de formas de token conocidas | Los canarios exactos dan cero falsos negativos sobre lo plantado. gitleaks sería un binario externo que hay que descargar y fijar en CI, sin nada más que comprobar aquí. ⚠️ **ASSUMPTION**: el coordinador acepta cambiar "gitleaks" por esto (§ 12, P1) |
| D8 | **Comprobaciones comunes en todo caso**: stdout solo con JSON-RPC, stderr solo con códigos fijos, ningún canario ni forma de token, ningún carácter oculto de L-03, cada parte ≤ 24 KiB, un rechazo ≤ 80 tokens estimados, la allowlist de campos, el repo intacto y ninguna trampa de `PATH` disparada | Es lo que exigen la historia y SEC-MCP-05, 06, 08 y 09 |
| D9 | **El comprobador de esquema falla cerrado**: una palabra clave fuera del subconjunto soportado es un error, no se ignora | Si `compact_schema` empieza a emitir otra palabra clave, la allowlist no puede pasar en silencio |
| D10 | **Autopruebas de mutación** en dos capas: puras (testkit, observaciones fabricadas) y reales (runner, observación real de un caso con la respuesta alterada) | Cumplen el Plan de Verificación: un traversal que pasa rompe la suite y un campo no declarado rompe la allowlist |
| D11 | **Gate nuevo** `.github/workflows/mcp-security-corpus.yml`, solo en Ubuntu, con filtro de rutas, el KPI en el job summary y un mínimo de casos ejecutados | macOS ya ejecuta el corpus en `lint and test (macos-latest)`, que es obligatorio. Windows lo ejecuta, sin bloquear, en `lint and test (windows-latest)`. Un job de Windows propio costaría una compilación en frío por PR. No se hace check obligatorio: lo decide Rene |
| D12 | **Todo el runner bajo `#![cfg(debug_assertions)]`** | `GITRAPTOR_PROFILE_DIR` solo existe en debug: en release, el arnés tocaría el perfil real (NFR-01). El mínimo de casos del workflow detecta un runner que no se compiló |
| D13 | **`pending` obligatorio** cuando un caso no corre en los tres SO, con la marca XP-42 | Un caso que se salta en un SO tiene que decir por qué. El informe lo cuenta aparte y **nunca** como rechazado |

## 4. Contratos

### 4.1 Formato del caso (JSON, esquema cerrado)

Ruta: `apps/cli/tests/mcp_corpus/cases/<id>.json`. El nombre del archivo es el `id`.

| Clave | Tipo | Regla |
|---|---|---|
| `id` | string | `^[a-z0-9]+(-[a-z0-9]+)*$`, ≤ 64, igual al nombre del archivo, único |
| `title` | string | En inglés, 1 a 160 caracteres |
| `threats` | string[] | No vacío. Ids de OWASP o de reglas (`MCP05`, `SEC-MCP-05`, `BR-MCP-VAL-005`) |
| `tier` | `"server"` \| `"engine"` | — |
| `platforms` | (`"macos"`\|`"linux"`\|`"windows"`)[] | No vacío y sin duplicados. `engine`, `symlinks`, `path_trap` y `parent: "agent"` excluyen `windows` (error si lo incluyen) |
| `pending` | string | Obligatorio si `platforms` no son los tres (p. ej. `"XP-42"`), prohibido si lo son |
| `setup` | objeto, opcional | `repo` y `other_repo`: `"none"`\|`"observed"`\|`"enabled"` (por defecto `none`). `worktrees`: `[{name, branch}]`, con `name` `^[a-z0-9-]{1,32}$`, sobre `repo`. `symlinks`: `[{at, to}]` (ubicaciones). `dirs`: [ubicación]. `path_trap`: bool. **En `server` debe faltar o ser igual al valor por defecto** |
| `session` | objeto, opcional | `cwd`: ubicación (por defecto `"repo"`). `env`: mapa con claves `^[A-Z_][A-Z0-9_]*$` y valores con marcadores. Las claves `GITRAPTOR_PROFILE_DIR` y `GITRAPTOR_AGENT_EXECUTABLES` están **prohibidas** (NFR-01). `parent`: `"unattributed"` (por defecto) \| `"agent"` (solo `engine`) |
| `send` | array no vacío | Cada elemento lleva exactamente una de estas formas: `{"call": "<tool>", "arguments"?: <cualquier JSON>, "repeat"?: 1..=200}`, `{"raw": "<línea>"}` (marcadores admitidos) o `{"message": {…}}` (objeto sin `id` ni `jsonrpc`, que pone el runner) |
| `expect` | objeto con una clave | `{"refusal": {"code": "<kebab>", "params"?: ["<nombre>"]}}`, `{"protocol_error": {"code": <int>, "message"?: "<s>", "field"?: "<s>"}}`, `{"invalid_request": {}}` o `{"ignored": {}}`. `ignored` exige que `send` solo lleve `raw` |
| `forbidden` | string[], opcional | Textos que no pueden aparecer en stdout ni en stderr (marcadores admitidos) |

**Ubicación**: `<ancla>` o `<ancla>/<rel>`, con `<ancla>` ∈ `root`, `repo`, `other_repo`, `home` o `wt-<name>` (declarado en `setup.worktrees`). Los componentes de `<rel>` son normales: ni vacíos, ni `.`, ni `..`, ni `\`, ni `:`. Si no, `CaseError::Location`.
**Marcadores**: `{root}`, `{repo}`, `{other_repo}`, `{home}`, `{repo_id:repo}`, `{repo_id:other_repo}` (exige ese repo en `observed` o `enabled`) y `{canary:<name>}`, con `<name>` ∈ `CANARY_NAMES`. Un marcador desconocido es `CaseError::Placeholder`.

Ejemplo (`server`):

```json
{
  "id": "status-cursor-traversal",
  "title": "A cursor that is a traversal path is malformed and reaches no engine",
  "threats": ["MCP05", "SEC-MCP-05", "BR-MCP-VAL-005"],
  "tier": "server",
  "platforms": ["macos", "linux", "windows"],
  "send": [{"call": "status", "arguments": {"cursor": "../../../../etc/passwd"}}],
  "expect": {"protocol_error": {"code": -32602, "message": "invalid-params", "field": "cursor"}}
}
```

Ejemplo (`engine`):

```json
{
  "id": "scope-symlink-into-not-enabled",
  "title": "A cwd reached through a symlink resolves to the real, not enabled repo",
  "threats": ["MCP02", "SEC-MCP-02"],
  "tier": "engine",
  "platforms": ["macos", "linux"],
  "pending": "XP-42",
  "setup": {"repo": "enabled", "other_repo": "observed",
            "symlinks": [{"at": "repo/into-other", "to": "other_repo"}]},
  "session": {"cwd": "repo/into-other"},
  "send": [{"call": "status"}],
  "expect": {"refusal": {"code": "repo-not-enabled"}},
  "forbidden": ["{other_repo}", "{repo_id:other_repo}"]
}
```

### 4.2 `crates/testkit/src/mcp_corpus/` (parte A)

Solo `std` y `serde_json`. Sin `unsafe` y sin dependencias nuevas.

```rust
// mod.rs
pub mod case;
pub mod judge;
pub mod report;
pub mod scan;
pub mod schema;
pub use case::{Case, CaseError, Expect, Platform, Tier, expand, load_dir, parse};
pub use judge::{Answer, Failure, Limits, Observation, Secrets, Stream, Verdict, judge};
pub use report::{Outcome, Report, Row};

// case.rs
pub const CANARY_NAMES: [&str; 8] = ["env-github-token", "env-aws-secret", "env-anthropic-key",
    "remote-userinfo", "remote-query", "dotenv-file", "commit-message", "git-extraheader"];
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)] pub enum Tier { Server, Engine }
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)] pub enum Platform { Macos, Linux, Windows }
impl Platform { pub fn current() -> Option<Self>; pub fn as_str(self) -> &'static str; pub const ALL: [Self; 3]; }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum RepoState { #[default] None, Observed, Enabled }
#[derive(Debug, Clone, PartialEq, Eq)] pub enum Anchor { Root, Repo, OtherRepo, Home, Worktree(String) }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Location { pub anchor: Anchor, pub rel: Vec<String> }
pub struct Roots { pub root: PathBuf, pub repo: PathBuf, pub other_repo: PathBuf, pub home: PathBuf }
impl Location { /// `Worktree(n)` is `root/wt-<n>`.
    pub fn resolve(&self, roots: &Roots) -> PathBuf; }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Worktree { pub name: String, pub branch: String }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Link { pub at: Location, pub to: Location }
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Setup { pub repo: RepoState, pub other_repo: RepoState, pub worktrees: Vec<Worktree>,
    pub symlinks: Vec<Link>, pub dirs: Vec<Location>, pub path_trap: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Parent { #[default] Unattributed, Agent }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session { pub cwd: Location, pub env: BTreeMap<String, String>, pub parent: Parent }
#[derive(Debug, Clone, PartialEq)]
pub enum Send { Call { tool: String, arguments: Option<Value>, repeat: u32 }, Raw(String), Message(serde_json::Map<String, Value>) }
#[derive(Debug, Clone, PartialEq)]
pub enum Expect { Refusal { code: String, params: Vec<String> },
    ProtocolError { code: i64, message: Option<String>, field: Option<String> }, InvalidRequest, Ignored }
#[derive(Debug, Clone, PartialEq)]
pub struct Case { pub id: String, pub title: String, pub threats: Vec<String>, pub tier: Tier,
    pub platforms: Vec<Platform>, pub pending: Option<String>, pub setup: Setup, pub session: Session,
    pub send: Vec<Send>, pub expect: Expect, pub forbidden: Vec<String> }
impl Case {
    /// Answers the case's messages must get: one per `call` repetition and one per `message`.
    pub fn expected_answers(&self) -> usize;
    pub fn runs_on(&self, platform: Platform) -> bool;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseError { Json(String), UnknownField(String), Missing(String),
    Invalid { field: String, why: String }, Location { field: String, why: String },
    Placeholder { field: String, name: String }, Tier(String), Platform(String),
    IdMismatch { id: String, file: String }, Duplicate(String), Io(String) }
impl std::fmt::Display for CaseError {} impl std::error::Error for CaseError {}
pub fn parse(text: &str) -> Result<Case, CaseError>;
/// Every `*.json` directly under `dir`, sorted by id. Reports every bad file, not only the first.
pub fn load_dir(dir: &Path) -> Result<Vec<Case>, Vec<(PathBuf, CaseError)>>;
pub fn expand(text: &str, vars: &BTreeMap<String, String>) -> Result<String, CaseError>;

// judge.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits { pub part_bytes: usize, pub refusal_bytes: usize, pub name_chars: usize }
#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct Secrets(pub Vec<(String, String)>); // (name, value)
#[derive(Debug, Clone, PartialEq)] pub struct Answer { pub tool: Option<String>, pub message: Value }
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    pub tools: Vec<Value>,          // `result.tools` of this session's `tools/list`
    pub answers: Vec<Answer>,       // in arrival order; handshake, tools/list and sentinel excluded
    pub stdout: Vec<String>,        // every stdout line, verbatim, handshake included
    pub stderr: String,
    pub sentinel_answered: bool,
    pub repo_changes: Vec<String>,  // `Change` (Display) left after the case's exceptions
    pub traps_fired: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Stream { Stdout, Stderr }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    NotRejected,
    WrongRejection { expected: String, got: String },
    MissingAnswers { expected: usize, got: usize },
    SessionDied,
    StdoutNotProtocol { line: usize },
    FieldNotAllowed { at: String },
    OverBudget { what: String, bytes: usize, limit: usize },
    SecretLeaked { secret: String, stream: Stream },   // the canary's NAME, never its value
    TokenShape { shape: &'static str, stream: Stream },
    HiddenCharacter { at: String },
    StderrNotFixed { line: usize },
    RepoChanged(Vec<String>),
    TrapFired(Vec<String>),
    Harness(String),
}
impl std::fmt::Display for Failure {}
#[derive(Debug, Clone, PartialEq, Eq)] pub enum Verdict { Rejected, Failed(Vec<Failure>) }
pub fn judge(case: &Case, observation: &Observation, secrets: &Secrets, limits: &Limits) -> Verdict;

// scan.rs
pub fn is_hidden(c: char) -> bool;
pub fn hidden_characters(value: &Value) -> Vec<String>;      // JSON pointers, keys included
pub fn secrets_in(text: &str, secrets: &Secrets) -> Vec<String>; // names found
pub fn token_shapes(text: &str) -> Vec<&'static str>;
pub fn stderr_fixed_codes(stderr: &str) -> Result<(), usize>;  // 1-based first bad line

// schema.rs
pub fn conforms(root: &Value, schema: &Value, value: &Value) -> Result<(), String>;

// report.rs
#[derive(Debug, Clone, PartialEq)] pub enum Outcome { Rejected, Failed(Vec<Failure>), Pending(String) }
#[derive(Debug, Clone, PartialEq)] pub struct Row { pub id: String, pub tier: Tier, pub outcome: Outcome }
#[derive(Debug, Clone, Default, PartialEq)] pub struct Report { pub os: String, pub rows: Vec<Row> }
impl Report {
    pub fn executed(&self) -> usize;       // rows not Pending
    pub fn rejected(&self) -> usize;
    pub fn pending(&self) -> usize;
    pub fn executed_in(&self, tier: Tier) -> usize;
    /// Tenths of a percent, rounded DOWN: 1000 only when every executed case was rejected; 0 with none.
    pub fn kpi_permille(&self) -> u32;
    pub fn summary_line(&self) -> String;
    pub fn markdown(&self) -> String;
    /// Err when nothing was executed or one executed case was not rejected.
    pub fn gate(&self) -> Result<(), String>;
}
```

**Reglas de `judge`** (en este orden, y acumula **todos** los fallos):

1. Cada línea de `stdout` es JSON; si no, `StdoutNotProtocol`. `stderr_fixed_codes` acepta líneas vacías o `raptor-mcp: [a-z][a-z-]*`; si no, `StderrNotFixed`.
2. `secrets_in` y `token_shapes` sobre cada línea de stdout y sobre stderr, con los canarios más los `forbidden` ya expandidos (el runner los mete en `Secrets` con el nombre `forbidden-<n>`) → `SecretLeaked` y `TokenShape`.
3. Si `!sentinel_answered`, `SessionDied`. Si `answers.len() != case.expected_answers()`, `MissingAnswers`.
4. Por cada respuesta, `hidden_characters` (en JSON parseado) → `HiddenCharacter`; y la allowlist:
   - **Error de protocolo**: claves ⊆ {`jsonrpc`, `id`, `error`}; `error` ⊆ {`code`, `message`, `data`}; `data` ⊆ {`field`}; `field` ⊆ {`untrusted`, `truncated`}, con `untrusted` ≤ `name_chars` caracteres. Toda la línea ≤ `part_bytes`.
   - **Rechazo** (`result.isError == true`): `result` ⊆ {`content`, `isError`}, sin `structuredContent`; un solo bloque {`type`, `text`}. `text` es JSON con `code`, `message` y `action` obligatorios, y además solo `params`, cuyas claves ⊆ `Expect::Refusal.params` (si el caso espera otra cosa, `params` debe faltar). `text` ≤ `refusal_bytes`.
   - **Éxito**: `result` ⊆ {`content`, `structuredContent`, `isError`}; `structuredContent` cumple `conforms` contra el `outputSchema` de `answer.tool` en `tools`, y si falta la herramienta o el esquema → `FieldNotAllowed`. `content[0].text` parseado es igual a `structuredContent`, y cada parte ≤ `part_bytes`.
5. **Veredicto**: con `Ignored`, `answers` tiene que estar vacío. Si no, se toma la primera respuesta que no es un éxito: si no hay ninguna, `NotRejected`; si no casa con `expect`, `WrongRejection`, con `got` igual a `refusal:<code>`, `protocol_error:<code>[:<message>]` o `invalid_request`, escapado y cortado a 80. `InvalidRequest` = `error.code == -32600` y `id` ausente o `null`.
6. `repo_changes` no vacío → `RepoChanged`. `traps_fired` no vacío → `TrapFired`.
7. Sin fallos → `Rejected`.

`Failure::Display` **nunca** imprime el valor de un secreto ni texto de la respuesta más allá de nombres de clave escapados (`escape_debug`) y cortados a 120.

**`is_hidden`**: `char::is_control`, U+061C, U+200B–U+200F, U+2028, U+2029, U+202A–U+202E, U+2060–U+2069, U+FEFF, U+FE00–U+FE0F, U+034F, U+115F, U+1160, U+3164, U+FFA0 y U+E0000–U+E007F.
**Formas de token**: `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`, `glpat-`, `xoxb-`, `xoxp-` y `sk-ant-`, cada una seguida de ≥ 16 `[A-Za-z0-9_-]`; `AKIA` seguido de 16 `[A-Z0-9]`; y `-----BEGIN ` con `PRIVATE KEY` en la misma línea. Sin crate de regex.
**`conforms`**: palabras clave soportadas: `$ref` (solo `#/$defs/<n>`), `$defs`, `type`, `properties`, `required`, `additionalProperties`, `items`, `enum`, `const`, `oneOf`, `anyOf`, `maxLength`, `minimum`, `maximum`, `format` (no se valida), `description` y `title`. Cualquier otra → `Err("unsupported keyword <k>")` (D9).
**`summary_line`**: `MCP security corpus (Q-MCP-18): {rejected}/{executed} rejected ({p}.{d} %) on {os}; {pending} pending`.
**`markdown`**: la primera línea es exactamente `<!-- mcp-corpus os={os} executed={e} rejected={r} pending={p} -->`. Después van `summary_line`, una tabla `| case | tier | outcome |` y, por cada fallo, su `Display`.

### 4.3 Runner `apps/cli/tests/mcp_corpus/` (parte B)

```rust
// mod.rs (public surface used by the contract test file)
pub fn cases_dir() -> PathBuf;                         // CARGO_MANIFEST_DIR/tests/mcp_corpus/cases
pub fn load_corpus() -> Result<Vec<Case>, Vec<String>>; // load_dir, errors rendered "<file>: <error>"
pub fn limits() -> Limits;  // MAX_MCP_PART_BYTES, MCP_REFUSAL_TOKENS * MCP_BYTES_PER_TOKEN, MAX_MCP_NAME_CHARS
pub struct CaseRun { pub observation: Observation, pub secrets: Secrets }
pub fn run_case(case: &Case) -> CaseRun;              // one real session on its own machine
pub fn run_corpus(cases: &[Case]) -> Report;          // pool of min(available_parallelism, 4)
pub fn write_report(report: &Report) -> PathBuf;      // CARGO_TARGET_TMPDIR/mcp-security-corpus.md
```

Qué hace `run_case`, en orden:

1. `Fixture::with_commit(&git_from_path())`. Siembra los **canarios** (valor `grc` + 29 hex derivados de `RandomState` y del id del caso):
   - `.env` sin seguimiento con `API_TOKEN=<dotenv-file>`;
   - `remote.origin.url = https://canary-user:<remote-userinfo>@example.invalid/x.git?private_token=<remote-query>`;
   - `http.extraHeader = Authorization: Bearer <git-extraheader>`;
   - un commit con `<commit-message>` en el mensaje.

   Hace lo mismo en `other_repo` (un commit con mensaje canario). Los canarios de entorno (`GITHUB_TOKEN`, `AWS_SECRET_ACCESS_KEY`, `ANTHROPIC_API_KEY`) van en el entorno de `raptor-mcp` **y** en el de la máquina, así que el daemon también los hereda. `canary-user` y `example.invalid` entran en `Secrets` con su propio nombre.
2. `setup`: crea los `worktrees` (rama más `add_worktree`), `dirs`, `symlinks` (`cfg(unix)`) y `path_trap` (`<root>/trap-bin/{git,raptor,raptor-mcp,sh}` con `canary::script`, que escriben `<root>/trap-fired/<name>`). Para `engine`, ejecuta `repo add` y `mcp enable` bajo pty (macOS `/usr/bin/script -q /dev/null …`; Linux `script -qec "<argv con comillas simples>" /dev/null`). Después espera la señal, no un reloj: `raptor status --json` lista exactamente los repos observados del `setup`. Con `parent: "agent"`, copia el binario de test a `<root>/raptor-fake-agent`. Resuelve los `{repo_id:…}` con `raptor status --json`.
3. Toma la huella **antes**. `server` usa `Exceptions::none()` y `GITRAPTOR_PROFILE_DIR=<root>/profile` (que existe y debe quedar idéntico: probar que el motor no se tocó es eso). `engine` usa `Exceptions::engine_profile("profile")`.
4. Lanza la sesión: `raptor-mcp` (de `sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp")`), con `env_clear()`, el entorno de la máquina, los canarios y el `env` del caso expandido (el del caso gana), y `current_dir(cwd)`. Con `parent: "agent"`, la lanza el agente simulado, como en `mcp_snapshot.rs:agent_command`. Hilos lectores de stdout y stderr con `mpsc`; **cada lectura con plazo de 15 s y el caso entero con 60 s**; al vencer, `kill` y `Harness("timeout …")`.
5. Protocolo: `initialize` (id 1, el mismo cuerpo que `handshake.rs`), `notifications/initialized`, `tools/list` (id 2, guarda `tools`). Después, los `send` en orden: `call` con ids desde 10 (`repeat` veces), esperando cada respuesta por id; `message` con su id; `raw` tal cual, sin esperar. Al final, un **centinela** `ping` (id 9999): las respuestas que llegan antes son `answers` (las de una llamada llevan `tool`) y la del centinela marca `sentinel_answered`. Después cierra stdin y espera la salida con plazo.
6. Huella **después**, `diff` y `Exceptions::filter`. Comprueba las trampas. En `Drop`, mata el daemon (`running_pid`; Unix `/bin/kill`, Windows `taskkill /PID <pid> /F`).

`run_corpus`: un caso que no corre en `Platform::current()` da `Outcome::Pending(case.pending)`. Cada caso se ejecuta bajo `catch_unwind`, y un pánico del arnés es `Failed([Harness(msg)])`, nunca un aborto del corpus. `os` = `Platform::current().map(as_str)` o `"other"`.

### 4.4 Lista de casos del corpus (parte C, 60 archivos)

Todos los `server` con `platforms` de los tres SO. Todos los `engine` con `["macos", "linux"]` y `pending: "XP-42"`. Los `params` de los rechazos sí se declaran. ⚑ = código que el experto **fija en la primera ejecución** (§ 8.4).

**`server` (43)**, `status`:

| id | `send` | `expect` |
|---|---|---|
| `status-param-repo` | `{"repo": "{other_repo}"}` | `-32602 invalid-params`, `field: repo`, `forbidden: ["{other_repo}"]` |
| `status-param-path-traversal` | `{"path": "../../other-repo"}` | ídem, `field: path` |
| `status-param-worktree` | `{"worktree": "../other-repo"}` | ídem, `field: worktree` |
| `status-param-hostile-name` | clave `"‮ignore-previous\u001b]52;c;cHduZWQ=\u0007"` | `-32602 invalid-params`, sin `field` (la comprobación de ocultos lo cubre) |
| `status-param-long-name` | clave de 4.096 `a` | `-32602 invalid-params`, sin `field` (`name_chars` lo cubre) |
| `status-cursor-traversal` | `{"cursor": "../../../../etc/passwd"}` | `-32602 invalid-params`, `field: cursor` |
| `status-cursor-absolute` | `"/etc/passwd"` | ídem |
| `status-cursor-unc` | `"\\\\server\\share\\x"` | ídem |
| `status-cursor-short` | 15 hex | ídem |
| `status-cursor-uppercase` | `"0123456789ABCDEF"` | ídem |
| `status-cursor-number` | `1234` | ídem |
| `status-cursor-object` | `{"untrusted": "x"}` | ídem |
| `status-cursor-bidi` | 15 hex + `‮` | ídem |
| `status-cursor-upload-pack` | `"--upload-pack=x"` | ídem |
| `status-cursor-ref-name` | `"refs/heads/main"` | ídem |
| `status-cursor-sha1` | 40 hex | ídem |
| `status-cursor-sha256` | 64 hex | ídem |
| `status-cursor-reflog` | `"HEAD@{1}"` | ídem |
| `status-cursor-glob-pathspec` | `":(glob)**/*"` | ídem |
| `status-cursor-depth-40` | array anidado 40 niveles | ídem |
| `status-cursor-oversized` | string de 2 MiB | ídem (tope de entrada: Mejora detectada) |

`snapshot`:

| id | `send` | `expect` |
|---|---|---|
| `snapshot-param-path` | `{"label": "a", "path": "/"}` | `-32602 invalid-params`, `field: path` |
| `snapshot-param-repo` | `{"label": "a", "repo": "{other_repo}"}` | ídem, `field: repo` |
| `snapshot-no-arguments` | sin `arguments` | ídem, `field: label` |
| `snapshot-label-missing` | `{}` | ídem, `field: label` |
| `snapshot-label-number` | `{"label": 3}` | ídem, `field: label` |
| `snapshot-label-too-long` | 65 `x` | rechazo `invalid-text`, `params: [field, max_chars]` |
| `snapshot-label-empty` | `""` | ídem |
| `snapshot-label-osc52` | `"a\u001b]52;c;cHduZWQ=\u0007b"` | ídem |
| `snapshot-label-nul` | `"a\u0000b"` | ídem |
| `snapshot-label-bidi` | `"‮abc"` | ídem |
| `snapshot-label-tags` | `"a󠁁b"` (U+E0041) | ídem |
| `snapshot-label-zero-width` | `"a​b"` | ídem |
| `snapshot-label-padded` | `" padded "` | ídem |
| `snapshot-label-zalgo` | `"a"` + 8 × U+0301 | ídem |

Herramienta y JSON-RPC:

| id | `send` | `expect` |
|---|---|---|
| `tool-unknown-push` | `call: "push"` | `-32602 unknown-tool` |
| `tool-unknown-reserved` | `call: "mcp.enable"`, `{"path": "{repo}"}` | ídem, `forbidden: ["{repo}"]` |
| `tool-unknown-traversal-name` | `call: "../status"` | ídem |
| `tool-unknown-bidi-name` | `call: "status‮"` | ídem |
| `jsonrpc-garbage-with-canary` | `raw: "{canary:env-github-token} not json"` | `ignored` |
| `jsonrpc-depth-200` | `raw`: `tools/call status` con `cursor` anidado 200 niveles | `ignored` (límite de serde: Mejora detectada) |
| `jsonrpc-wrong-shape` | `message: {"method": 7}` | `invalid_request` |
| `jsonrpc-arguments-not-object` | `message: {"method": "tools/call", "params": {"name": "status", "arguments": "x"}}` | `invalid_request` ⚑ |
| `jsonrpc-resources-list` | `message: {"method": "resources/list"}` | `protocol_error`, `-32601` ⚑ |

**`engine` (17)**:

| id | `setup` | `session` | `send` | `expect` y `forbidden` |
|---|---|---|---|---|
| `scope-cwd-outside-observed` | repo `enabled`, dirs `root/downloads` | cwd `root/downloads` | `status` | `not-in-observed-worktree`; `{repo}`, `{repo_id:repo}` |
| `scope-nothing-observed` | (nada) | cwd `repo` | `status` | `not-in-observed-worktree`; `{repo}` |
| `scope-repo-not-enabled` | repo `observed` | cwd `repo` | `status` | `repo-not-enabled`; `{repo}`, `{repo_id:repo}` |
| `scope-subfolder-of-not-enabled` | repo `enabled`, other `observed`, dirs `other_repo/src` | cwd `other_repo/src` | `status` | `repo-not-enabled`; `{other_repo}`, `{repo_id:other_repo}` |
| `scope-symlink-into-not-enabled` | ejemplo § 4.1 | | | |
| `scope-sibling-prefix` | repo `enabled`, dirs `root/repo-x` | cwd `root/repo-x` | `status` | `not-in-observed-worktree`; `{repo_id:repo}` |
| `scope-linked-worktree-of-not-enabled` | repo `observed`, other `enabled`, worktree `feat-a` | cwd `wt-feat-a` | `status` | `repo-not-enabled`; `{repo_id:repo}` |
| `env-claude-project-dir` | repo `enabled`, dirs `root/downloads` | cwd `root/downloads`, `CLAUDE_PROJECT_DIR={repo}` | `status` | `not-in-observed-worktree`; `{repo_id:repo}` |
| `env-git-dir` | repo `enabled`, other `observed` | cwd `other_repo`, `GIT_DIR={repo}/.git`, `GIT_WORK_TREE={repo}` | `status` | `repo-not-enabled`; `{repo_id:repo}`, `{repo_id:other_repo}` |
| `env-home-and-xdg` | repo `enabled`, dirs `root/downloads` | cwd `root/downloads`; `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` y `XDG_STATE_HOME` = `{repo}` | `status` | `not-in-observed-worktree`; `{repo_id:repo}` |
| `env-path-trap` | repo `enabled`, other `observed`, `path_trap` | cwd `other_repo`, `PATH={root}/trap-bin` | `status` | `repo-not-enabled` (y ninguna trampa) |
| `status-cursor-unknown-to-engine` | repo `enabled` | cwd `repo` | `status`, cursor `0123456789abcdef` | `invalid-cursor` |
| `status-rate-limited` | repo `enabled` | cwd `repo` | `status` × `repeat: 80` | `rate-limited`, `params: [retry_after_s]` |
| `snapshot-repo-not-enabled` | repo `observed` | cwd `repo` | `snapshot` `{"label": "probe"}` | `repo-not-enabled` ⚑ |
| `snapshot-outside-observed` | repo `enabled`, dirs `root/downloads` | cwd `root/downloads` | `snapshot` `{"label": "probe"}` | `not-in-observed-worktree` ⚑ |
| `snapshot-unattributed` | repo `enabled` | cwd `repo` | `snapshot` `{"label": "probe"}` | `unattributed` |
| `snapshot-agent-repo-not-enabled` | repo `observed` | cwd `repo`, `parent: agent` | `snapshot` `{"label": "probe"}` | `repo-not-enabled` ⚑ |

## 5. File & Project Topology: slices disjuntos

Orden: **0** (coordinador, antes del baseline) → (**A** ‖ **B** ‖ **C**) → (**D** ‖ **E**). Ningún archivo está en dos slices. B puede empezar a la vez que A porque los stubs del slice 0 fijan las firmas, pero sus tests de integración solo pasan cuando A está hecho.

### Slice 0 — pruebas de contrato (coordinador; Nx: `gitraptor-testkit`, `gitraptor-cli`)

- `crates/testkit/tests/mcp_corpus.rs` (nuevo)
- `apps/cli/tests/mcp_security_corpus.rs` (nuevo)

### Slice A — lógica del corpus en el testkit (Nx: `gitraptor-testkit`)

- `crates/testkit/src/mcp_corpus/mod.rs` (nuevo)
- `crates/testkit/src/mcp_corpus/case.rs` (nuevo)
- `crates/testkit/src/mcp_corpus/judge.rs` (nuevo)
- `crates/testkit/src/mcp_corpus/scan.rs` (nuevo)
- `crates/testkit/src/mcp_corpus/schema.rs` (nuevo)
- `crates/testkit/src/mcp_corpus/report.rs` (nuevo)
- `crates/testkit/src/lib.rs` (una línea `pub mod mcp_corpus;` más una viñeta en la cabecera)

### Slice B — runner con binarios reales (Nx: `gitraptor-cli`)

- `apps/cli/tests/mcp_corpus/mod.rs` (nuevo)
- `apps/cli/tests/mcp_corpus/machine.rs` (nuevo: fixture, canarios, setup, pty, agente, daemon)
- `apps/cli/tests/mcp_corpus/session.rs` (nuevo: proceso `raptor-mcp`, lectores con plazo, centinela)

### Slice C — casos del corpus (datos; Nx: `gitraptor-cli`)

- `apps/cli/tests/mcp_corpus/cases/*.json` (60 archivos nuevos, § 4.4)

### Slice D — gate de CI

- `.github/workflows/mcp-security-corpus.yml` (nuevo)

### Slice E — documentación

- `docs/requirements/features/mcp/technical-stories/INF-MCP-001-corpus-seguridad-mcp.md`
- `docs/requirements/features/mcp/technical-stories.md`
- `docs/requirements/features/mcp/dev-specs/INF-MCP-001-dev-spec.md` (nuevo, `DS-INF-MCP-001`)
- `docs/requirements/backlog.md`
- `docs/requirements/release-plan.md`
- `docs/requirements/release-status.md` (generado)
- `docs/architecture/xplat-pendientes.md`

## 5.1 Fuera de los límites

No se tocan: todo el código de producción (`apps/mcp/`, `apps/cli/src/`, `crates/api/`, `crates/core/`, `crates/git/`, `crates/policy/`), los manifiestos Cargo, `Cargo.lock`, `.config/`, `tools/ci/`, los `project.json`, los demás workflows (en especial `repo-intact.yml`), `docs/ARTIFACTS.md` y los E2E existentes `mcp_allowlist.rs` y `mcp_snapshot.rs`. No hacen falta dependencias nuevas.

## 6. Matriz de plataformas

| Aspecto | macOS | Linux | Windows |
|---|---|---|---|
| Nivel `server` (43) | Soportado; se verifica en local y en `lint and test (macos-latest)` (obligatorio) | Soportado; se verifica en el gate nuevo y en `lint and test (ubuntu-latest)` | Soportado; se verifica en `lint and test (windows-latest)`, sin bloquear, como el resto de `cargo test` en Windows |
| Nivel `engine` (17) | Soportado; en local y en `lint and test (macos-latest)` | ⚠️ **ASSUMPTION**: soportado (el daemon lee el cwd del par con `/proc` en `crates/core/src/channel/peer.rs`, y `script -qec` funciona en los E2E de Guardrails), pero `mcp.status` nunca se ha ejecutado de extremo a extremo en Linux. Se verifica en el gate nuevo | `Pending` en el informe, marcado **XP-42**: canal por named pipe, comandos reservados desde una consola y agente simulado sin validar |
| `symlinks`, `path_trap`, `parent: agent` | Soportado | Soportado | El cargador lo rechaza (`CaseError::Platform`) si un caso lo pide |
| Ruta UNC como cwd | — | — | Fila pendiente, XP-42 (solo existe en Windows) |

Si el primer run de Ubuntu muestra un fallo **del daemon** en Linux (no del arnés), el experto **no** lo arregla: deja los casos afectados en `["macos"]` con `pending: "XP-42"`, baja `MIN_EXECUTED` y lo reporta como hallazgo. Esto lo tiene que aprobar el coordinador (§ 12, P2).

## 7. NFR

- **Tiempo**: el corpus completo tarda ≤ 120 s en Ubuntu CI y ≤ 90 s en un Mac de desarrollo (⚠️ **ASSUMPTION**, a medir en el primer run). nextest corta a los 240 s. Cada lectura tiene un plazo de 15 s y cada caso de 60 s. No hay `sleep` fijos.
- **Aislamiento (NFR-01)**: cada caso tiene su propia `Fixture`, su perfil y su daemon. Ningún caso toca este repo, el perfil real ni la config global de Git. Todo el runner va bajo `cfg(debug_assertions)`.
- **Observabilidad**: la línea `summary_line` en stdout (con `--nocapture`), el informe markdown en `CARGO_TARGET_TMPDIR/mcp-security-corpus.md` y el job summary del gate. Un fallo dice el id del caso y su motivo, sin valores de secretos.
- **Seguridad**: el arnés lanza procesos (`script`, el agente simulado, `raptor`, `raptor-mcp`) con argv fijo y entorno limpio; no usa shell salvo `script -qec` en Linux, que es el patrón que ya existe. El informe escapa todo texto que procede de una respuesta. **Pasar por `security-expert`**: ejecución de procesos, rutas de los casos, salida de terminal en el informe y manejo de canarios.

## 8. Plan de pruebas (contrato)

### 8.1 Pruebas de contrato en rojo (las escribe el coordinador antes del baseline)

**`crates/testkit/tests/mcp_corpus.rs`**. Son puras, corren en todos los SO y fabrican el JSON y las `Observation` dentro del propio test:

| Test | Afirma |
|---|---|
| `a_case_file_parses_into_its_model` | Los dos ejemplos de § 4.1 dan el `Case` esperado (tier, plataformas, `Setup`, `Expect`, `expected_answers() == 1`) |
| `a_case_with_an_unknown_field_or_an_escaping_location_is_rejected` | Una clave desconocida (raíz, `setup` y `expect`) da `UnknownField`. `cwd: "repo/../other_repo"` y `"repo//x"` dan `Location`. Un marcador desconocido da `Placeholder`. `GITRAPTOR_PROFILE_DIR` en `env` da `Invalid` |
| `a_server_tier_case_with_engine_setup_is_rejected` | `server` con `setup.repo: "enabled"` o `parent: "agent"` da `Tier`. `engine` con `windows` da `Platform`. Plataformas parciales sin `pending` dan `Invalid`. `ignored` con un `call` da `Invalid` |
| `a_traversal_case_that_gets_through_is_not_rejected` | La observación del ejemplo `server` con un **éxito** fabricado (`structuredContent` válido) da `Failed` con `NotRejected`. Con el `-32602` correcto da `Rejected`. Con `-32602 field: label` da `WrongRejection` |
| `a_refusal_with_an_undeclared_field_breaks_the_allowlist` | Un rechazo `invalid-text` con una clave extra (`"repo_path"`) da `FieldNotAllowed`. Un `params` no declarado, también. Un `error.data` con una clave extra, también |
| `a_success_with_a_field_outside_its_output_schema_breaks_the_allowlist` | Un éxito cuyo `structuredContent` lleva una propiedad que el `outputSchema` (con `additionalProperties: false`) no declara da `FieldNotAllowed`. Un esquema con una palabra clave no soportada hace que `conforms` devuelva `Err` |
| `a_planted_secret_in_an_answer_or_in_stderr_is_found_without_printing_it` | Un canario en stdout o en stderr da `SecretLeaked` con su **nombre**, y el `Display` del fallo **no** contiene el valor. `ghp_` + 36 caracteres da `TokenShape` |
| `a_part_over_its_budget_fails_the_case` | Un texto de éxito de `part_bytes + 1` bytes da `OverBudget`. Un rechazo de `refusal_bytes + 1` también |
| `stderr_with_anything_but_fixed_codes_fails_the_case` | `"raptor-mcp: internal-error"` pasa. `"panicked at src/x.rs"` da `StderrNotFixed { line: 1 }` |
| `hidden_characters_in_an_answer_fail_the_case` | U+202E, U+E0041, U+200B y ESC, en un valor y en una clave, dan `HiddenCharacter` con su puntero JSON |
| `a_repo_change_or_a_fired_trap_fails_the_case` | `repo_changes` no vacío da `RepoChanged`. `traps_fired` no vacío da `TrapFired`. Una observación sin el centinela da `SessionDied` |
| `the_report_prints_the_kpi_and_the_gate_fails_below_100_percent` | 3 rechazados de 3: `kpi_permille() == 1000`, `gate()` Ok, `summary_line` contiene `3/3 rejected (100.0 %)` y `markdown()` empieza por `<!-- mcp-corpus os=test executed=3 rejected=3 pending=0 -->`. 2 de 3: `666`, `gate()` Err con el id del fallido |
| `an_empty_corpus_fails_the_gate` | Un `Report` vacío o con todo `Pending`: `gate()` Err |
| `a_pending_case_is_counted_apart_and_never_as_rejected` | 1 rechazado y 1 pendiente: `executed == 1`, `rejected == 1`, `pending == 1` y `kpi_permille == 1000` |

**`apps/cli/tests/mcp_security_corpus.rs`**, con `#![cfg(debug_assertions)]` y `mod mcp_corpus;`:

| Test | Afirma |
|---|---|
| `fake_agent_entry` | Ayudante (`cfg(unix)`), **no es criterio**. El cuerpo es el de `mcp_allowlist.rs:fake_agent_entry`, en línea, sin stub: sin la variable no hace nada, así que pasa en el baseline |
| `every_case_file_is_valid_unique_and_named_after_its_id` | `load_corpus()` Ok, ≥ 40 casos, los dos tiers presentes y al menos un caso con cada `Expect` |
| `the_corpus_is_fully_rejected` | `run_corpus(&load_corpus())`, `write_report` y `println!` de `summary_line`. Después, `gate()` Ok y `executed_in(Server) ≥ 1`; en macOS y Linux, también `executed_in(Engine) ≥ 1` |
| `an_injected_pass_of_a_traversal_case_fails_the_suite` | `run_case("status-cursor-traversal")` real da `Rejected`. Se sustituye `answers[0].message` por un éxito fabricado y da `Failed ⊇ [NotRejected]`. Un `Report` con esa fila tiene `gate()` Err y `summary_line` con `0/1 rejected (0.0 %)` |
| `an_undeclared_field_in_a_real_refusal_breaks_the_allowlist` | `run_case("snapshot-label-too-long")` real da `Rejected`. Si se inyecta `"repo_path"` en el JSON del bloque de texto, da `FieldNotAllowed`. `run_case("status-param-repo")` real con `error.data.secret` inyectado, también |

### 8.2 Stubs para que compilen (los escribe el coordinador con la parte 0; los archivos son de A y de B)

Los stubs **fallan en abierto**: el rojo sale del comportamiento, no de un pánico. Las pruebas demuestran justamente que el arnés no es permisivo.

- **testkit**: los tipos completos de § 4.2. `parse` devuelve `Err(CaseError::Json("stub"))` y `load_dir`, `Ok(vec![])`. `expand` devuelve el texto tal cual. `judge` devuelve siempre `Verdict::Rejected`. `scan::*` devuelve vacío u `Ok(())`. `conforms` devuelve `Ok(())`. `Report::{executed, rejected, pending, executed_in, kpi_permille}` devuelven 0, `summary_line` y `markdown` devuelven `String::new()` y `gate` devuelve `Ok(())`. `Platform::current` es real (3 líneas con `cfg!`). En `crates/testkit/src/lib.rs`, `pub mod mcp_corpus;`.
- **runner**: `cases_dir` es real. `load_corpus` devuelve `Ok(vec![])`. `limits` es real. `run_case` devuelve `CaseRun { observation: Observation::default(), secrets: Secrets::default() }` y `run_corpus`, `Report::default()`. `write_report` devuelve la ruta sin escribir.

### 8.3 Aislamiento

Repos, perfil y `HOME` temporales (`Fixture`). Nunca el perfil real (`cfg(debug_assertions)`). El daemon muere en `Drop`. Las trampas de `PATH` solo escriben dentro de `<root>`.

### 8.4 Cuándo el experto se detiene (BLOQUEO)

- Un caso ⚑ que responde con otro código **de rechazo**: el experto fija el código observado en el JSON y lo anota en el PR. Si en cambio responde con un **éxito**, o la sesión muere, es un hallazgo de seguridad: `--event blocked`, sin tocar el servidor.
- Un caso `engine` que falla en Linux por el daemon: § 6.
- Una diferencia en la huella que no es determinista: se investiga. **Nunca** se añade una excepción sin aprobación.

### 8.5 Comando de aceptación

```bash
cargo build -p gitraptor-cli -p gitraptor-mcp --bins
cargo test -p gitraptor-testkit --test mcp_corpus
cargo test -p gitraptor-cli --test mcp_security_corpus -- --nocapture
cargo clippy -p gitraptor-testkit -p gitraptor-cli --all-targets -- -D warnings
cargo fmt --all --check
node tools/status/release-status.mjs --check
```

## 9. Workflow `mcp-security-corpus.yml` (parte D)

- `on`: `pull_request` con `paths`: `apps/mcp/**`, `crates/api/**`, `crates/core/src/channel/**`, `crates/testkit/**`, `apps/cli/tests/mcp_security_corpus.rs`, `apps/cli/tests/mcp_corpus/**`, `.github/workflows/mcp-security-corpus.yml`, `Cargo.lock` y `rust-toolchain.toml`; además `push` a `main` y `workflow_dispatch`. `permissions: contents: read`. `concurrency` por número de PR (o `run_id`), cancelando solo en PR.
- Un job `corpus`, con nombre `mcp security corpus (ubuntu-latest)`, en `ubuntu-latest` y `timeout-minutes: 30`. Pasos:
  1. `actions/checkout` con el **mismo SHA** que `repo-intact.yml`.
  2. `rustup show`.
  3. `Swatinem/rust-cache` con el mismo SHA y `save-if: ${{ github.ref == 'refs/heads/main' }}`.
  4. `rm -f target/tmp/mcp-security-corpus.md` (para no leer un informe viejo de la caché).
  5. `cargo build -p gitraptor-cli -p gitraptor-mcp --bins` (`raptor` y `raptor-mcp` frescos y juntos).
  6. `cargo test -p gitraptor-testkit --test mcp_corpus`.
  7. `cargo test -p gitraptor-cli --test mcp_security_corpus -- --nocapture`.
  8. **KPI al job summary** (`if: ${{ !cancelled() }}`, `shell: bash`, `env: MIN_EXECUTED: "<casos con linux al aterrizar: 60>"`): falla si no existe el informe; hace `cat` del informe a `$GITHUB_STEP_SUMMARY`; lee `executed` y `rejected` de la primera línea; falla con `::error::` si `executed < MIN_EXECUTED` o `rejected != executed`.
- Sin `env` `RUST_*` ni `CARGO_*`. Sin macOS ni Windows. Una cabecera de comentario explica D11 y que **no** es un check obligatorio (lo decide Rene).

## 10. Not Built (deferred)

- **gitleaks**: se añade cuando haga falta detectar secretos que no se plantaron, por ejemplo en la revisión por release de SEC-MCP-12.
- **Nivel `engine` en Windows**: se añade cuando se valide XP-42 (named pipe, comandos reservados desde una consola y agente simulado).
- **Job de macOS o Windows en el gate nuevo**: se añade cuando sobren runners de macOS o el gate pase a ser obligatorio.
- **Check obligatorio de `main`**: cuando Rene lo decida.
- **PID reutilizado (`identity-unverified`)**: necesita un paso de control de procesos en el formato. Se añade con la historia que lo pida.
- **Cliente directo `cli` bajo un agente (SEC-MCP-01)**: cuando llegue S-01 (M4). Hoy fallaría por diseño.
- **50 conexiones de un agente (SEC-MCP-03)**: con US-MCP-009.
- **Repo de otro uid**: necesita un segundo usuario del SO; `mcp_status_full` ya lo cubre en core.
- **Respuesta de 3.000 archivos**: es un éxito acotado, no un rechazo. La cubre `mcp_status_full`. Se añadiría un `expect: bounded` si el KPI llega a contar "contenidos".
- **Rechazo del tope de 1 MiB y 32 niveles**: cuando `raptor-mcp` lo aplique (Mejora detectada).
- **Casos de `safe_commit`, `safe_rebase`, `create_worktree`, `explain_history`, `check_conflicts`, `undo` y `acknowledge`**: con la historia dueña de cada herramienta (filas pendientes).
- **Fuzzing**: fuera de alcance según la historia.
- **Crate `jsonschema`**: cuando los `outputSchema` usen palabras clave fuera del subconjunto (`conforms` falla cerrado y avisa).
- **Un test de nextest por caso**: cuando el KPI se agregue fuera del proceso de test.
- **Migrar `mcp_allowlist.rs` y `mcp_snapshot.rs` al runner**: cuando se toquen esos E2E.

## 11. Documentación (parte E)

1. **Historia** (`INF-MCP-001-corpus-seguridad-mcp.md`):
   - frontmatter con `status: partially-implemented`, `updated: <fecha>` y `specs: [DS-INF-MCP-001]`;
   - la línea `> Dev Spec:` apuntando a `dev-specs/INF-MCP-001-dev-spec.md`;
   - la línea `Implementado en: PR #<n>.` (forma canónica de `US-MCP-004`);
   - una sección `### Estado del corpus` con una tabla de grupo de casos, historia dueña y estado. **Implementado**: transversales de `status` y `snapshot`, ámbito, entorno, JSON-RPC, rate limit. **Pendiente**: PID reutilizado, S-01, SEC-MCP-03, otro uid, UNC (XP-42), 3.000 archivos, tope de entrada, y las filas de US-MCP-007, 009, 010, 016, 017, 018 y 019;
   - un `**Falta**:` con Linux y Windows marcados como *Pendiente: etapa de validación multiplataforma* (XP-42);
   - si P1 se aprueba, "gitleaks" pasa a "escáner propio de canarios".
2. `technical-stories.md`: en la fila de INF-MCP-001, la última columna pasa a "Parcialmente implementada (#n)".
3. `backlog.md`, línea "Implementación": añadir "INF-MCP-001 parcialmente (#n: corpus, arnés y gate; casos de herramientas futuras pendientes)".
4. `release-plan.md`, fila `INF-MCP-001`: `Pend` pasa a `Parcial`.
5. `xplat-pendientes.md`: una fila nueva **XP-42**. Plataformas: Linux y Windows. Historia: INF-MCP-001. Qué falta: el nivel `engine` del corpus en Windows, UNC como cwd y la primera ejecución verde del nivel `engine` en Ubuntu. Canal: CI y la máquina Windows. Estado: pendiente. ⚠️ El número puede chocar con otra rama: el experto confirma el máximo al hacer rebase.
6. **Dev Spec** `DS-INF-MCP-001` (`dev-specs/INF-MCP-001-dev-spec.md`): con el frontmatter de `US-MCP-008-dev-spec.md` (`type: dev-spec`, `story: INF-MCP-001`, `stack: rust`, `author: rust-architect`, `status: partially-implemented`) y el resumen de § 3, § 4, § 6 y § 10 de este Brief. Preferible generarlo con `/aadd-devspec` a partir de este Brief.
7. `node tools/status/release-status.mjs` para regenerar `release-status.md`.
8. **No** se toca `docs/ARTIFACTS.md`. ADR-MCP-001 § 9 y `non-functional.md` (SEC-MCP-08) **solo** si se aprueba P1, y con una enmienda fechada, no reescribiendo el texto.

## 12. Preguntas abiertas y supuestos

- **P1** ⚠️ **ASSUMPTION**: sustituir "gitleaks sobre respuestas y stderr" (historia, ADR-MCP-001 § 9 y SEC-MCP-08) por el escáner propio de D7. Cambia el texto de un requisito, así que lo aprueba el coordinador o el security-expert.
- **P2** ⚠️ **ASSUMPTION**: el nivel `engine` funciona en Ubuntu al primer intento. Si no, se aplica la regla de § 6 (macOS solo más XP-42) en lugar de arreglar el daemon en esta rama.
- **P3**: el corpus es un test normal de `apps/cli`, así que ya corre en `lint and test (macos-latest)` y `(ubuntu-latest)`, que **son checks obligatorios**. En la práctica, ya bloquea el merge en macOS y Linux aunque el workflow nuevo no sea obligatorio. Rene debería saberlo.
- **P4**: el filtro de rutas del gate nuevo no incluye `crates/core` fuera de `channel`. Una regresión allí la detecta `lint and test`, no el gate nuevo.
- **P5**: los códigos ⚑ se fijan en la primera ejecución. Un éxito en cualquiera de ellos es un BLOQUEO.
- **P6** (herramienta del flujo): el baseline separa el rojo de las pruebas del contrato leyendo el JUnit. Los nombres de nextest (`gitraptor-testkit::mcp_corpus`) no llevan la ruta del archivo, y `belongsToDeclared` de `deliver-contract.mjs` no los asocia. Es probable que B1 salte con el rojo esperado. Salidas: `--accept-red-baseline` o registrar el baseline a mano (`--record`).

## Traspaso

`rust-expert`: ejecuta este Brief por slices (§ 5) **después** de que el coordinador escriba las pruebas de § 8.1 y los stubs de § 8.2 y mida el baseline. El contrato es `docs/dev-briefs/inf-mcp-001-mcp-security-corpus.contract.json`. No decidas nada que este Brief ya fija: si algo no encaja, para y repórtalo (§ 8.4). `security-expert` revisa el slice B y el escáner del slice A antes del PR.

## 13. Ajustes del coordinador (2026-10-09) — plan aprobado

Decisión del orquestador (2026-10-09), aprobada por el coordinador de Orca:

1. **XP-42** (no XP-41, ocupado por #219) para el nivel `engine` en Windows; si al rebasar otro lo tomó, el siguiente libre.
2. **Escáner de secretos propio** aceptado en lugar de gitleaks. Condición: los canarios tienen forma de secreto real (token `ghp_…`, clave `AKIA…`, cabecera PEM de clave privada, contraseña en URL) y se siembran en repo, config y entorno. La enmienda de la ficha se valida con `nassa-architect:architect` y se registra como "Decisión del orquestador (2026-10-09), validada por Arquitecto".
3. **Tope de entrada (1 MiB / profundidad 32)** no aplicado por `raptor-mcp`: no basta una nota. Se añaden casos marcados `known_gap` (campo opcional del caso: referencia al hueco, p. ej. `"known_gap": "US-MCP-005 seguimiento: tope de entrada"`). `Outcome::KnownGap(ref)`: el informe los cuenta aparte, no entran al KPI como rechazados ni en el denominador; si el caso resulta rechazado, el gate falla pidiendo quitar la marca (el hueco se cerró y el caso pasa a exigir rechazo). No se toca `apps/mcp`.
4. Workflow aparte, solo ubuntu, barato, **no obligatorio**.
5. Baseline B1: `--record`/hallazgo de plugins en el PR.
