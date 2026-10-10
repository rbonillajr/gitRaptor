---
id: DS-US-GRD-018
title: "Dev Spec — US-GRD-018 y US-GRD-019: política de autoría de los commits y quién ejecutó frente a a nombre de quién entra"
type: dev-spec
status: partially-implemented
feature: guardrails
domain: GRP
story: US-GRD-018
created: 2026-10-06
updated: 2026-10-08
related:
  stories: [US-GRD-018, US-GRD-019, US-GRD-001, US-GRD-005, US-GRD-007, US-GRD-009, US-GRD-010, US-GRP-002, US-GRP-007, US-GRP-009]
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRP-005, ADR-GRP-007, ADR-GRP-012, ADR-GRP-013, ADR-GRP-016]
  rules: [BR-AUTH-005, BR-AUTH-003, BR-CONS-001, BR-CONS-004, BR-CALC-001, BR-EDGE-001, BR-EDGE-004]
  nfrs: [NFR-01, NFR-02]
tags: [guardrails, autoria-commits, co-authored-by, agents-commit, human-author, flexible, actor-s4, raptor-events, pista-inferida, capacidades]
---

# Dev Spec — US-GRD-018 y US-GRD-019: política de autoría de los commits

Plano compacto (AADD ligero) de [US-GRD-018](../user-stories/US-GRD-018-autoria-commits-persona-y-agente.md) (la política: decidir el commit) y de [US-GRD-019](../user-stories/US-GRD-019-quien-ejecuto-y-a-nombre-de-quien.md) (la presentación y la validación de la pista `inferred`). Las dos historias comparten la tabla de identidades, la lectura de trailers y el contrato, así que van en **una sola Dev Spec y se implementan en dos PR** (§ 9). La regla es [BR-AUTH-005](../business-rules.md) (D6 del BRD, Rene Bonilla, 2026-10-06). El marco lo fijan [ADR-GRP-012](../../../../architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md), Enmienda (2026-10-06, autoría de commits), [ADR-GRP-013](../../../../architecture/decisions/ADR-GRP-013-modelo-eventos-atribucion.md), Enmienda (2026-10-06, autoría declarada), y [ADR-GRD-003](../../../../architecture/decisions/ADR-GRD-003-motor-decision-contrato.md), Enmienda (2026-10-06, US-GRD-018), que deja esta Dev Spec. La forma de extender sigue [ADR-GRP-016](../../../../architecture/decisions/ADR-GRP-016-extension-registro-capacidades.md).

**Invariante**: GitRaptor **nunca** reescribe el autor ni añade el trailer. Solo decide, avisa y registra.

## 1. Decisiones

Cada fila es una **Decisión del orquestador (2026-10-06), validada por el Arquitecto** (`nassa-architect:architect`, una consulta). La columna de la derecha recoge los ajustes que pidió, ya incorporados (cuatro bloqueantes: D2, D9, D11 y rellenar esta columna).

| # | Decisión | Ajuste de la validación |
|---|---|---|
| D1 | **Clave de configuración** `policies.commitAuthorship` (§ 4): `{ "mode": "agents-commit" \| "human-author" \| "flexible", "onAgentCommit": "deny" \| "warn" }`. `onAgentCommit` solo vale con `human-author` (por defecto `deny`); con otro `mode` es clave fuera de lugar → diagnóstico y se ignora. Niveles admitidos (`x-gitraptor-levels`): equipo (suelo y worktree), perfil y local. Es la **primera clave soportada de `policies`**: deja de producir `policy-not-supported` | Sin ajuste |
| D2 | **Combinación** (BR-CONS-001, BR-CALC-001, Q-GRD-20): orden total `flexible` < `agents-commit` < `human-author`+`warn` < `human-author`+`deny`. Valor efectivo = el **máximo** entre el valor por defecto (`agents-commit`) y las fuentes que la fijan. **Relajar por debajo del valor por defecto (`flexible`) solo se lee del suelo** (equipo en la rama principal, confirmado y legible; un cambio del suelo sigue Q-GRD-21). Un `flexible` en el worktree, el perfil o el local se ignora con el diagnóstico `relaxation-not-allowed`. Con el suelo `parcial` o ilegible, nunca se baja de `agents-commit` | **Ajuste (bloqueante)**: el borrador dejaba que un nivel personal pusiera `flexible` sin clave en el equipo, en contra de Q-GRD-20 |
| D3 | **`warn` no es `ask`** (pregunta del Arquitecto en ADR-GRP-012 § 4). `ask` se aplica hoy como `deny` (`ask-unavailable`, S-GRD-9), así que bloquearía. `warn` se traduce a `effect = allow` más una entrada en un campo nuevo **`notices[]`** de `Decision` (misma forma que `reasons[]`). El orden `deny > ask > allow` no cambia y un aviso nunca sube el efecto. Si `appliedEffect` no es `allow` (otra regla deniega, o `ask` aplicado como `deny`), los avisos se descartan. El cliente del hook imprime la plantilla del aviso por stderr y sale con 0. En el registro la decisión es `allow` con aviso y **no cuenta** en el KPI de acciones bloqueadas | **Ajuste**: descartar los avisos con cualquier `appliedEffect` distinto de `allow`, no solo con `deny` |
| D4 | **El actor entra en la condición de las reglas de autoría** (enmienda de ADR-GRD-003 § 1). Solo endurecen y solo cuando el actor es un agente (detectado o registrado). Con "sin atribuir" ninguna regla de autoría deniega ni avisa (BR-EDGE-004). Ninguna otra regla lee el actor | Sin ajuste |
| D5 | **Actor S4 en `guard.evaluate`** (ADR-GRD-003 § 4; pendiente de US-GRD-001 asignado a US-GRD-005/006, **lo adelanta esta historia**). El daemon toma el pid del cliente del hook por las credenciales del canal, sube hasta el `git` antecesor más cercano (identidad `(pid, inicio)` verificada en cada paso) y **sigue subiendo por encima de él** (agente → shell → `git`): un antecesor que es el proceso de una sesión detectada (US-GRP-007, `detect`) → `agent(kind, detected)`; si no, agente registrado en ese worktree (US-GRP-009) → `agent(kind, registered)`; si no, `unattributed`. Modo degradado: siempre `unattributed`. El código vive en `crates/core/src/guardrails/actor.rs` para que US-GRD-005 y US-GRD-006 lo reutilicen | **Ajuste**: la búsqueda de la sesión sube por encima del `git`; US-GRD-005 y US-GRD-006 usan `actor.rs` y no lo rehacen (anotado en sus historias) |
| D6 | **Dónde se evalúa** (ADR-GRD-002 § 1, fila Commit): se añaden los dispatchers `pre-commit` y `commit-msg` al conjunto instalado (plantilla `TEMPLATE_VERSION = 2`; `raptor hook` acepta la 1 y la 2; una instalación con la plantilla 1 se actualiza al reinstalar). `pre-commit`: solo `human-author`+`deny` con actor agente (corta antes del mensaje). `commit-msg`: todas las reglas, con los trailers del archivo del mensaje. `reference-transaction` `prepared`: **segunda línea** para `--no-verify` (§ 5). Una decisión por operación (ADR-GRD-003 § 6): si la identidad de la operación ya tiene decisión de autoría, la segunda línea la reutiliza | Sin ajuste (ADR-GRD-001 y ADR-GRD-002 § 1 ganan su enmienda) |
| D7 | **Hechos de contenido** (ADR-GRD-003 § 1): el cliente del hook lee el mensaje (archivo de `commit-msg`, acotado a 64 KiB, `O_NOFOLLOW`, solo archivo regular) o el commit nuevo (segunda línea, con gix aislado) y calcula con la función pura de `crates/policy` solo `{ coauthors: [{ agent: AgentKind? }], trailerTable: u32 }`. **Al daemon nunca llega el mensaje, ni nombres ni correos**. Mensaje ilegible o mayor de 64 KiB → `coauthors = []` (si el actor es un agente, `agents-commit` deniega con causa `message-unreadable`) | **Ajuste**: `commit-msg` recibe el mensaje **antes** del *cleanup*; el parser aplica `core.commentChar` y el modo de `--cleanup`/`commit.cleanup`, y el corpus de conformidad cubre esos casos |
| D8 | **Tabla versionada de identidades** (`crates/policy/src/authorship/agents.rs`, § 3). Hoy una sola fila, Claude Code. El formato admite Codex y Cursor (D2 del BRD) sin cambiar el código: una fila por agente. `trailerTable` (versión de la tabla) va en `configRef` y en el registro | **Ajuste**: se declara que cualquier herramienta basada en el SDK de Anthropic que firme con ese correo cuenta como Claude Code |
| D9 | **Rebase, cherry-pick, revert, merge y amend** (§ 5): la política gobierna la operación **commit** del catálogo (BR-VAL-002): `commit` (también `--amend`) y el commit de fusión de `merge`. Rebase, cherry-pick y revert **no** se evalúan: conservan el autor y los trailers del original y GitRaptor no exige trailers a commits que no creó en esa operación. Quien quiera impedir que un agente reescriba usa `permissions` (`rebase` en `deny`) | **Ajuste (bloqueante)**: la segunda línea no usa una lista de subcomandos evaluados (se salta con alias o `commit-tree` + `update-ref`), sino la lista inversa (§ 5.3) |
| D10 | **Contrato de eventos** (US-GRD-019, ADR-GRP-016 § 1): capacidad nueva **`events.authorship`**. Con ella, `GitEventView` lleva `authorship` (autor, committer y co-autores con `agent` opcional, todo `Untrusted`) e `InferredAgent` lleva `trailer: confirmed \| unconfirmed`. Sin ella, la forma de siempre. `raptor-mcp` no la pide, así que la vista MCP no recibe nombres ni correos (ADR-GRP-013, Enmienda autoría declarada) | Sin ajuste |
| D11 | **Capacidad `guard.authorship`** (ADR-GRP-016 § 1): cubre los dos campos nuevos de un método existente, `EvaluateParams.authorship` (petición) y `Decision.notices[]` (respuesta). El cliente del hook la pide en `connection.accept` y **solo envía `authorship` si el daemon se la concede** (`EvaluateParams` es `deny_unknown_fields`). Un daemon sin ella evalúa como hoy: sin reglas de autoría ni avisos, y el commit sigue las demás reglas (no endurece ni relaja respecto a hoy) | **Ajuste (bloqueante)**: el borrador (`guard.notices`) solo cubría la respuesta; un hook nuevo frente a un daemon viejo habría recibido `INVALID_PARAMS` y bloqueado commits |
| D12 | **Registro** (BR-CONS-004, ADR-GRD-006): con `human-author` y `flexible`, y en toda denegación de autoría, la entrada lleva el actor, los tipos de agente de los co-autores, si había trailer, la decisión y el oid del commit cuando la operación llega a `reference-transaction`. **El autor y el committer no se copian al registro**: se muestran uniendo por oid con la autoría declarada del evento (ADR-GRP-013), que ya los guarda. Así el registro sigue sin guardar contenido del commit (ADR-GRP-012 § 4). **Huecos declarados**: una denegación no tiene oid ni evento, así que su entrada muestra quién lo intentó, la decisión y su motivo, y "autor no disponible: el commit no llegó a crearse" (el criterio no pide el autor); si la autoría del evento ya no existe, "autor no disponible". **Decisión del PO (2026-10-06)**: la unión por oid cumple el punto 4 de BR-AUTH-005, aclarado en ese sentido (el autor y el committer se pueden ver, no se copian). Copiarlos se descartó: duplica nombres y correos en otro almacén y en la exportación (BR-24) y rompe D7. Un rebase o un gc no rompen la unión, porque es con el evento y no con el objeto de Git. Restricción para el futuro: una retención de los eventos nunca más corta que la del registro (90 días). El contrato no cambia | **Ajuste**: declarar los dos huecos y llevar la unión por oid al PO (confirmada) |
| D13 | **Dos PR**: PR-A = US-GRD-018 (D1 a D9, D11, D12); PR-B = US-GRD-019 (D10, presentación y pista). PR-B depende de PR-A por la tabla y el parser de trailers. Si US-GRD-005 (registro) no está en `main` cuando se implemente PR-A, las entradas de D12 quedan pendientes con dueño US-GRD-005 y se dice en el PR; el resto de PR-A no depende del registro | Sin ajuste |

## 2. Forma (archivos que se tocan)

Cada pieza nueva va en un archivo propio de su módulo (ADR-GRP-016); los archivos centrales solo ganan una línea.

| Pieza | Archivo | PR |
|---|---|---|
| Tabla de identidades y parser de trailers (puro, sin E/S) | `crates/policy/src/authorship/{mod,agents,trailers}.rs` (nuevo); `crates/policy/src/lib.rs` (+1 línea `pub mod authorship;`) | A |
| Reglas `authorship.*` y combinación de D2 | `crates/policy/src/guard/authorship.rs` (nuevo); `crates/policy/src/guard/mod.rs` (llamada desde `evaluate` y campo `actor`/`authorship` en `Context`/`Facts`) | A |
| Clave `policies.commitAuthorship`, schema y niveles | `crates/policy/src/settings/{model,schema,document}.rs` | A |
| Contrato: `Decision.notices`, `EvaluateParams.authorship`, `Actor` en el contexto | `crates/api/src/guard.rs` | A |
| Capacidad `guard.authorship` | `crates/api/src/methods/guard.rs` (su `Group`) | A |
| Actor S4 | `crates/core/src/guardrails/actor.rs` (nuevo); `crates/core/src/guardrails/{mod,evaluate}.rs` | A |
| Dispatchers `pre-commit` y `commit-msg`, plantilla 2 | `crates/core/src/guardrails/{constants,install}.rs`; `apps/cli/src/bin/raptor-hook.rs` | A |
| Cliente del hook: lectura del mensaje y del commit, aviso por stderr | `apps/cli/src/commands/guard.rs` (o el módulo de `raptor hook` donde viva hoy) | A |
| Mensajes del hook (deny y aviso) | `apps/cli/i18n/{en,es}/guard.txt` | A |
| Entrada de registro de autoría | Archivo de US-GRD-005 en `crates/core/src/guardrails/` (si existe; D13) | A |
| Autoría declarada del evento y estado de la pista | `crates/api/src/messages.rs` (`GitEventView`, `InferredAgent`, tipos `DeclaredAuthorship`, `GitIdentity`, `CoAuthor`, `TrailerCheck`) | B |
| Capacidad `events.authorship` | `crates/api/src/methods/events.rs` | B |
| Lectura de la autoría del commit nuevo al observar y evidencia `trailer` | `crates/core/src/observe.rs` y el almacén de eventos de `crates/core/src/profile/` | B |
| Política efectiva al observar (para "sin pista con `human-author`") | `crates/core/src/observe.rs` (lee la configuración efectiva por el cargador de TS-GRD-001) | B |
| `raptor events` (texto y `--json`) | `apps/cli/src/commands/events.rs`; `apps/cli/i18n/{en,es}/events.txt` | B |
| Tests | § 7 | A y B |
| Documentación | Este archivo (estado y verificación); `docs/architecture/ipc-contract*` si documenta `guard.evaluate` y `git.event` | A y B |

## 3. Identidades del trailer por agente

`crates/policy/src/authorship/agents.rs`, tabla `AGENT_TRAILERS: &[AgentTrailer]`, con `TABLE_VERSION: u32 = 1`:

```rust
pub struct AgentTrailer {
    pub agent: AgentKind,            // ClaudeCode hoy; Codex y Cursor cuando entren (D2)
    pub emails: &'static [&'static str],        // comparación exacta, ASCII sin mayúsculas
    pub name_prefixes: &'static [&'static str], // prefijo tras NFC y plegado de mayúsculas
    pub example: &'static str,       // el trailer de ejemplo del mensaje de denegación
}
```

| Agente | Correo | Prefijo del nombre | Ejemplo |
|---|---|---|---|
| Claude Code | `noreply@anthropic.com` | `Claude` | `Co-Authored-By: Claude <noreply@anthropic.com>` |
| Codex, Cursor | — (filas que se añaden con su adaptador, D2; cambian `TABLE_VERSION`) | — | — |

- **Reconocido** = el correo coincide **y** el nombre empieza por el prefijo. Así entran "Claude", "Claude Opus 4.x" o "Claude Sonnet 5.x" con el correo de Anthropic, y no entra "Claudia <claudia@x.com>". Un co-autor que no encaja queda **sin tipo**.
- **Parser** (`trailers.rs`): sigue las reglas de `git interpret-trailers` para el **último párrafo** del mensaje (líneas `clave: valor`, clave sin distinguir mayúsculas, continuación con espacio, líneas de comentario `#` fuera con `commit.cleanup` por defecto). Solo se miran las claves `Co-Authored-By`. Valor `Nombre <correo>`; uno malformado cuenta como co-autor sin tipo. Tope: 64 KiB de mensaje y 32 co-autores.
- **Conformidad**: un test compara el parser con `git interpret-trailers --parse` sobre un corpus (trailer al final, en medio, con comentarios, con `Signed-off-by` mezclado, CRLF, sin línea en blanco previa, mayúsculas distintas).
- El adaptador de cada agente (ADR-GRP-012 § 1) es el dueño de su fila; la tabla vive en `crates/policy` porque la usa la función pura.

## 4. Esquema de la política en la configuración del repo

Archivo de equipo `.gitraptor/settings.json` (ADR-GRP-007; el perfil y el local usan la misma forma):

```json
{
  "policies": {
    "commitAuthorship": { "mode": "human-author", "onAgentCommit": "warn" }
  }
}
```

| Campo | Tipo | Por defecto | Validación |
|---|---|---|---|
| `policies.commitAuthorship.mode` | `enum` `agents-commit`, `human-author`, `flexible` | `agents-commit` (sin la clave) | Otro valor → la fuente queda `parcial` (D12 de ADR-GRP-007) y la clave no aplica |
| `policies.commitAuthorship.onAgentCommit` | `enum` `deny`, `warn` | `deny` | Solo con `mode = human-author`; con otro → diagnóstico `key-out-of-place` y se ignora |

- **Relajar a `flexible` solo desde el suelo** (Q-GRD-20, D2): en el worktree, el perfil o el local se ignora con `relaxation-not-allowed`; `human-author` sí se admite en cualquier nivel porque endurece.
- Schema con `x-gitraptor-levels: [team, profile, local]` y el subconjunto cerrado de palabras clave de TS-GRD-001.
- El estado de protección (`guard.status`) muestra la política efectiva y de qué fuente sale.
- Modo degradado: la clave del suelo legible se lee, pero no cambia nada porque el actor es `unattributed` (enmienda de ADR-GRD-003 § 4).

## 5. Decisión por operación

### 5.1 Reglas (`crates/policy/src/guard/authorship.rs`)

| Política efectiva | Actor agente con su trailer | Actor agente sin su trailer | Actor `unattributed` |
|---|---|---|---|
| `agents-commit` | `allow` | `deny`, `authorship.trailer-required` (params: agente, ejemplo de la tabla) | `allow`, sin regla |
| `human-author` + `deny` | `deny`, `authorship.human-author` | `deny`, `authorship.human-author` | `allow`, sin regla |
| `human-author` + `warn` | `allow` + aviso `authorship.human-author` | `deny`, `authorship.trailer-required` + aviso (BR-AUTH-005: "con avisar… siempre que cumpla `agents-commit`") — el aviso se descarta por D3 | `allow`, sin regla |
| `flexible` | `allow` | `allow` | `allow` |

- "Su trailer" = un co-autor reconocido con el **mismo** `AgentKind` que el actor. El trailer de otro agente no cuenta.
- `level` de la razón: la fuente que fija el máximo (`floor`, `worktree`, `profile`, `local`) o `system` para el valor por defecto.

### 5.2 Mapeo a `allow` / `ask` / `deny` (ADR-GRD-003 § 3)

| Resultado de la política | `effect` | `appliedEffect` | `reasons[]` | `notices[]` | Salida del hook |
|---|---|---|---|---|---|
| Pasa | `allow` | `allow` | — | — | 0 |
| Deniega | `deny` | `deny` | la regla | — | 1, plantilla del motivo |
| Avisa (`warn`) | `allow` | `allow` | — | la regla | 0, plantilla del aviso por stderr |

Nunca se produce `ask`. Con otras reglas (mínimo, permisos, US-GRD-009) manda el máximo de siempre.

### 5.3 Por operación de Git

| Operación | Hook que decide | Qué se evalúa |
|---|---|---|
| `git commit` | `pre-commit` (solo `human-author`+`deny`), `commit-msg` (todo) | El mensaje del archivo de `commit-msg` |
| `git commit --amend` | Igual que `commit` | El mensaje nuevo. Enmendar el commit de una persona sin trailer **es** un commit del agente: con `agents-commit` exige el trailer |
| `git commit --no-verify` | `reference-transaction` `prepared` | El commit nuevo (gix aislado), si la operación no tenía ya decisión de autoría |
| `git merge` con commit de fusión (también `pull` que fusiona) | `commit-msg` (Git lo ejecuta en `merge`), segunda línea con `--no-verify` | El commit de fusión, no los commits fusionados |
| `git merge` fast-forward | — | Nada: no hay commit nuevo |
| `git rebase`, `cherry-pick`, `revert`, `am` | — | Nada (D9). Los gobiernan `permissions` y US-GRD-009 |
| Escrituras internas de la Time Machine | — | Desactivan los hooks (ADR-TMC-002 § 2) |

**Cómo sabe la segunda línea que es un commit** (lista inversa, ajuste del Arquitecto): con actor agente, una línea de `refs/heads/*` (o de `HEAD` separado) con **un único commit nuevo cuyo primer padre es `viejo`** (commit, fusión) o cuyos padres son los de `viejo` (amend) **se evalúa siempre**, salvo que el subcomando del `git` antecesor más cercano sea **con certeza** `rebase`, `cherry-pick`, `revert` o `am`. El subcomando se lee de la línea de órdenes del proceso tras saltar las opciones globales (`-C`, `-c`, `--git-dir`, `--work-tree`, `--namespace`, `--exec-path`, `-p`, …), **solo para clasificar** (no se guarda ni se envía: ADR-GRD-003 § 1, "lo que nunca guarda: argv"). Un alias (`git ci`), `git -c alias.x=commit x`, `commit-tree` + `update-ref` o una línea de órdenes ilegible **se evalúan**. Con varios commits nuevos en `viejo..nuevo` no se evalúa (no es la forma de un commit) y queda el diagnóstico `authorship-unclassified`. Residuo declarado: un agente que escribe con `update-ref` un rango de varios commits escapa a la segunda línea.

## 6. Contrato

### 6.1 Decisión (`crates/api/src/guard.rs`, PR-A)

```rust
pub struct Decision {
    // … campos de siempre (ADR-GRD-003 § 3)
    /// Rules that warn without changing the effect (capability `guard.authorship`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Reason>,
}

pub struct EvaluateParams {
    // … campos de siempre
    /// Commit facts the hook client derived from the message or the new commit (never the text).
    /// Sent only when the daemon granted `guard.authorship`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorship: Option<AuthorshipFacts>,
}

pub struct AuthorshipFacts {
    pub coauthors: Vec<Option<AgentKind>>, // one entry per Co-Authored-By; None = not recognised
    pub trailer_table: u32,
    pub unreadable: bool,                  // message missing, too large or not a regular file
}
```

- Reglas nuevas: `authorship.trailer-required`, `authorship.human-author`. Causas: `message-unreadable`, `authorship-unclassified`. Ningún código de error nuevo.
- El actor **no** viaja en `EvaluateParams`: lo resuelve el daemon (D5). El cliente no puede declararlo.

### 6.2 Eventos (`crates/api/src/messages.rs`, PR-B, capacidad `events.authorship`)

```rust
pub struct GitEventView {
    // … campos de siempre
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorship: Option<DeclaredAuthorship>, // only for events that create a commit
}

pub struct DeclaredAuthorship {
    pub author: GitIdentity,
    pub committer: GitIdentity,
    pub coauthors: Vec<CoAuthor>,
}
pub struct GitIdentity { pub name: Untrusted, pub email: Untrusted }
pub struct CoAuthor { pub name: Untrusted, pub email: Untrusted,
                      #[serde(default, skip_serializing_if = "Option::is_none")] pub agent: Option<AgentKind> }

pub struct InferredAgent {
    pub kind: AgentKind,
    pub session_id: String,
    pub trailer: TrailerCheck,                  // confirmed | unconfirmed; contradicted → no `inferred`
}
```

- Sin `events.authorship` la conexión recibe la forma del protocolo 9 (sin `authorship` ni `trailer`). Un evento antiguo sin evidencia `trailer` se lee como `unconfirmed`.
- **Persona y agente en `raptor events`**: la línea se compone en el cliente con las reglas de ADR-GRP-012, Enmienda § 3: "commit de \<autor\> con \<agentes co-autores\> · \<worktree\>", más "· ejecutado por \<agente\> (detectado)" si el actor es un agente que no figura como co-autor, o "· sin atribuir" / "· sin atribuir; inferido: … (confirmado por el trailer)". `--json` lleva `actor`, `authorship` e `inferred` por separado.
- Cadenas nuevas (en/es) en `events.txt`: `events.commit_by` ("commit by {author}" / "commit de {author}"), `events.commit_with` ("with {agents}" / "con {agents}"), `events.run_by` ("run by {agent}" / "ejecutado por {agent}"), `events.inferred_confirmed` ("confirmed by the trailer" / "confirmado por el trailer"), `events.no_trailer` ("no trailer" / "sin trailer").

### 6.3 Mensajes del hook (`guard.txt`, PR-A; M-05)

| Clave | es | en |
|---|---|---|
| `guard.reason.authorship-trailer-required` | En este repo el commit de un agente lleva su trailer (política «agents-commit»). Añade al final del mensaje: «{example}» | In this repo an agent's commit carries its trailer ("agents-commit" policy). Add at the end of the message: «{example}» |
| `guard.reason.authorship-human-author` | En este repo los commits los hace la persona (política «human-author», {level}) | In this repo commits are made by the person ("human-author" policy, {level}) |
| `guard.notice.authorship-human-author` | Aviso: en este repo los commits los hace la persona (política «human-author»). El commit sigue | Warning: in this repo commits are made by the person ("human-author" policy). The commit goes ahead |

Plantillas fijas, parámetros etiquetados y saneados; nunca mencionan la excepción ni cómo desactivar la política.

## 7. Criterios de aceptación verificables

Repos, remotos, perfiles y daemons temporales (NFR-01); Git, `raptor` y `raptor-hook` reales; el agente es `raptor-fake-agent` como Claude Code (patrón de `apps/cli/tests/claude_sessions.rs`) o `raptor agent register`; sin esperas fijas. Suites nuevas: `apps/cli/tests/guard_us_grd_018.rs` y `apps/cli/tests/events_us_grd_019.rs` (se niegan a correr sin *debug assertions*, como la de US-GRD-001).

| Criterio | Test |
|---|---|
| US-GRD-018 esquema 1 · sin política y con `agents-commit`: con trailer pasa (autor = identidad de Git del usuario, trailer intacto en `git log`); sin trailer no pasa, el motivo nombra `agents-commit` y trae el ejemplo | `agents_commit_needs_the_agents_trailer` (4 filas) |
| US-GRD-018 · commit sin agente sin trailer pasa | `an_unattributed_commit_needs_no_trailer` |
| US-GRD-018 · `human-author` con `deny` y con `warn` (texto en en y es; salida 0 con aviso) | `human_author_blocks_or_warns` |
| US-GRD-018 · `flexible` pasa sin trailer | `flexible_lets_the_commit_through` |
| US-GRD-018 · un nivel local `flexible` no relaja el `human-author` del equipo; el motivo nombra la configuración del equipo | `a_personal_level_does_not_relax_the_team_policy` |
| D2 · un `flexible` en el worktree, el perfil o el local se ignora (`relaxation-not-allowed`) y rige `agents-commit`; un `flexible` del suelo confirmado sí aplica | `only_the_floor_relaxes_to_flexible` |
| D5 · actor detectado (`raptor-fake-agent`) y registrado (`raptor agent register`) deciden igual; un trailer falso en un commit sin agente no cambia nada | `detected_and_registered_agents_are_the_actor`, `a_fake_trailer_does_not_make_an_actor` |
| D6 · `--no-verify` lo cubre la segunda línea; una sola entrada por operación; un alias (`git ci --no-verify`), `-c alias.x=commit` y `commit-tree` + `update-ref` también | `no_verify_is_caught_by_the_second_line`, `aliases_and_plumbing_are_caught_by_the_second_line` |
| D9 · `--amend` de un commit humano por el agente exige el trailer; rebase y cherry-pick del agente no se evalúan; merge con commit de fusión sí; fast-forward no | `amend_merge_rebase_and_cherry_pick` |
| D7 · mensaje mayor de 64 KiB o enlace → `message-unreadable`; el daemon nunca recibe el mensaje (espía en el canal de test) | `the_message_never_reaches_the_daemon` |
| D3 · modo degradado: ninguna regla de autoría deniega | `degraded_mode_does_not_apply_authorship_rules` |
| D12 · registro con `human-author` y `flexible`; la entrada no guarda nombres ni correos y muestra el autor y el committer uniendo por oid con el evento; una denegación muestra "autor no disponible: el commit no llegó a crearse"; los avisos y `flexible` no cuentan en el KPI | `authorship_entries_in_the_decision_log` (si US-GRD-005 está en `main`; si no, pendiente) |
| § 3 · parser frente a `git interpret-trailers --parse` (también `core.commentChar` y los modos de `--cleanup`); tabla de identidades | `crates/policy` `authorship::tests::*` |
| § 5.1 · tabla completa de reglas y combinación de D2 (prueba de propiedades: un nivel personal nunca baja el máximo) | `crates/policy` `guard::authorship::tests::*` |
| § 4 · schema, `onAgentCommit` fuera de lugar, valor desconocido → `parcial`; `commitAuthorship` ya no da `policy-not-supported` | `crates/policy` `settings::document::tests::commit_authorship_*` |
| D11 · sin `guard.authorship` el cliente no envía `authorship`, no hay `notices` en el cable y un daemon viejo no rechaza el commit con `INVALID_PARAMS` | `crates/api` `legacy_protocols.rs` y `crates/core/tests/guard_evaluate.rs` |
| US-GRD-019 · "commit de Ana Pérez con Claude Code · feat-x"; `--json` separa actor y autoría | `events_us_grd_019::agent_commit_shows_both` |
| US-GRD-019 · sin agente: "commit de Ana Pérez · main" y "sin atribuir" | `events_us_grd_019::an_unattributed_commit_does_not_repeat_the_author` |
| US-GRD-019 · pista `confirmed`, `unconfirmed`, `contradicted` (sin `inferred`); el actor sigue "sin atribuir" | `events_us_grd_019::the_inferred_hint_is_checked_against_the_trailer` (3 filas) |
| US-GRD-019 · con `human-author` no hay pista; cambiar la política después no reescribe eventos | `events_us_grd_019::human_author_records_no_hint` |
| US-GRD-019 · `flexible`, agente sin trailer: ejecutado por Claude Code, a nombre de Ana, sin trailer | `events_us_grd_019::an_agent_commit_without_trailer_shows_the_difference` |
| ADR-GRP-013 (autoría declarada) · sobrevive a un reinicio; una corrección cambia el actor y no la autoría; prueba de propiedades con autores y trailers aleatorios: el actor no cambia; sin `events.authorship` no hay nombres | `crates/core/tests/` (archivo de eventos existente) y `crates/api` |

**Comandos de cierre** (una vez, antes del PR): `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`. Durante el desarrollo: `cargo test -p gitraptor-policy authorship`, `cargo test -p raptor --test guard_us_grd_018` (nombres de paquete según `Cargo.toml`).

## 8. Pendientes y fuera de alcance

| Pendiente | Dueño |
|---|---|
| Filas de Codex y Cursor en la tabla de identidades | Sus adaptadores (D2 del BRD) |
| La misma decisión por MCP (`safe_commit`) | US-GRD-016 (BR-CONS-002) |
| Presentación en el Cockpit | Historia del Cockpit por escribir (PO de Cockpit) |
| Exportar las entradas de autoría | BR-24 (Fase 3) |
| Que US-GRD-005 y US-GRD-006 reutilicen `actor.rs` | Sus Dev Specs (D5) |
| Agente que escribe con `update-ref` un rango de varios commits escapa a la segunda línea | Residuo declarado (§ 5.3) |
| Un commit sin trailer hecho con `commit-tree` (no toca ninguna ref, así que no hay hook) y luego aplicado con `git cherry-pick` escapa por D9 | Residuo declarado (§ 11; en `voluntary-skips`) |
| Registro de autoría si US-GRD-005 no está en `main` | US-GRD-005 (D13) |
| Que una pista confirmada pase a ser atribución | PO o Rene (ADR-GRP-012, Enmienda § 2) |
| Ratificar la pista `inferred` | Rene Bonilla (US-GRD-019) |
| Windows: leer la línea de órdenes del `git` antecesor (`NtQueryInformationProcess`, en `winsys`) y los dispatchers nuevos. Hasta entonces la línea de órdenes es ilegible y se evalúa: cada paso de un `rebase` o `cherry-pick` de un agente tiene la forma de un commit y, con `agents-commit`, se podría bloquear (fail-closed). Linux: validarlo en una máquina real | **Pendiente: etapa de validación multiplataforma** |
| Agente no detectado ni registrado escapa a la política (R-GRD-2) | Residuo declarado (BR-AUTH-005) |
| ~~Segunda línea frente a `--no-verify`~~ **Hecho** (§ 11): `git commit --no-verify` ya no salta `agents-commit` ni `human-author`. Verificado en macOS; Linux implementado con `/proc` y cubierto por el CI de ubuntu; Windows no tiene canal todavía | — |
| Agente **registrado** (US-GRP-009) sin proceso detectado como actor de `guard.evaluate` (D5, desviación de PR-A): los registros viven en el almacén del daemon, fuera del hilo de la conexión; hace falta una copia compartida de solo lectura. La prueba `detected_and_registered_agents_are_the_actor` queda pendiente en su parte "registrado" | US-GRP-009 / US-GRD-005 |
| Nivel **local** de `commitAuthorship`: no hay lector de `settings.local.json` | US-GRP-013 |
| `flexible` de extremo a extremo: un suelo con configuración de equipo queda sin confirmar al instalar (Q-GRD-23); se confirma con US-GRD-014 | US-GRD-014 |
| Un agente `other` no tiene fila en la tabla de identidades: con `agents-commit` no podría cumplir (hoy no se resuelve como actor). Cuando entre el agente registrado hará falta una regla explícita para `other` | US-GRP-009 / US-GRD-005 |
| Una clave desconocida dentro de `policies.commitAuthorship` da `policy-not-supported` (deja la fuente `parcial`, correcto) aunque la política sí está soportada; el diagnóstico debería nombrar la subclave | Deuda del validador (TS-GRD-001) |
| ~~Reinstalar un repo con la plantilla 1 para pasar a la 2~~ **Hecho** (§ 11): `raptor guard install` actualiza en el sitio | — |

## 9. Orden de implementación

1. **PR-A (US-GRD-018)**: `crates/policy` (tabla, parser, reglas, clave) → `crates/api` (contrato, `guard.authorship`) → `crates/core` (actor S4, dispatchers, segunda línea) → `apps/cli` (cliente del hook, mensajes) → suites. Se implementa con `/nassa-core:implement --autonomous DS-US-GRD-018` limitado a PR-A.
2. **PR-B (US-GRD-019)**: `crates/api` (autoría declarada, `events.authorship`) → `crates/core` (observación y evidencia) → `apps/cli` (`raptor events`) → suite.

## 10. Estado de PR-A (2026-10-07)

Implementado en la rama `feat/US-GRD-018-commit-authorship-policy` (D1 a D5, D6 sin segunda línea, D7, D8, D11; D12 pendiente porque US-GRD-005 no está en `main`, D13).

| Pieza | Dónde |
|---|---|
| Tabla de identidades y parser de trailers (conformidad con `git interpret-trailers --parse`, `core.commentChar`, modos de limpieza) | `crates/policy/src/authorship/` |
| Reglas y combinación de D2 (prueba de propiedades) | `crates/policy/src/guard/authorship.rs` |
| Clave `policies.commitAuthorship`, diagnósticos `relaxation-not-allowed` y `key-out-of-place` | `crates/policy/src/settings/` |
| Contrato (`notices`, `authorship`, operación `commit`, hooks `pre-commit`/`commit-msg`, capacidad `guard.authorship`) | `crates/api/src/guard.rs`, `crates/api/src/methods/guard.rs` |
| Actor S4, política efectiva (suelo, worktree, perfil), plantilla 2 | `crates/core/src/guardrails/{actor,authorship,evaluate,hook,constants,install,registry}.rs` |
| Mensajes en/es y dispatcher | `apps/cli/src/guard.rs`, `apps/cli/i18n/{en,es}/guard.txt`, `apps/cli/src/bin/raptor-hook.rs` |
| Suites | `apps/cli/tests/guard_us_grd_018.rs`, `crates/core/tests/guard_evaluate.rs` |

**D6 aplazado en parte**: la segunda línea (`reference-transaction` frente a `--no-verify`, § 5.3) no entra en PR-A; la prueba `no_verify_is_caught_by_the_second_line` queda pendiente (§ 8).

**Decisiones del orquestador (2026-10-07), validadas por el Arquitecto** (`nassa-architect:architect`, una consulta; sus ajustes ya están incorporados) tomadas al implementar (registradas también en el PR):

1. El actor reutiliza `crates/core/src/channel/requester.rs::resolve` en lugar de un segundo recorrido de la ascendencia.
2. En modo degradado o con otra instancia, `pre-commit` y `commit-msg` salen 0 sin mensaje: el actor es "sin atribuir" y el `reference-transaction` del mismo commit ya avisa del modo degradado. **Ajuste del Arquitecto**: con un daemon antiguo sin `guard.authorship` nada más avisaría, así que el hook imprime una línea (`guard.notice.authorship-unavailable`, en/es) y sale 0.
3. La lectura de `commit.cleanup` y `core.commentChar` usa la configuración del usuario (no aislada): solo sirve para leer el mensaje como Git lo limpiará; la decisión no depende de ella más allá de los trailers.
4. Claves desconocidas dentro de `policies.commitAuthorship` siguen el validador existente: `policy-not-supported` y fuente `parcial` (el Arquitecto lo acepta como deuda, § 8).
5. El actor es solo el agente detectado (o el marcado por el ejecutor); el registrado queda pendiente (§ 8). **Ajuste del Arquitecto**: anotado como desviación de D5 con dueño.

**Criterios del § 7 cubiertos por PR-A**: `agents_commit_needs_the_agents_trailer`, `an_unattributed_commit_needs_no_trailer`, `human_author_blocks_or_warns`, `a_personal_level_does_not_relax_the_team_policy`, `only_the_floor_relaxes_to_flexible` (e2e con el perfil; el suelo confirmado en `crates/core/tests/guard_evaluate.rs`), `degraded_mode_does_not_apply_authorship_rules`, `an_unreadable_message_denies_the_agent` (mayor de 64 KiB), `a_template_1_install_keeps_working`, `amend_merge_rebase_and_cherry_pick` (sin la parte de `--no-verify`), `crates/policy` `authorship::tests::*`, `guard::authorship::tests::*`, `settings::document::tests::commit_authorship_*`, compatibilidad D11 en `crates/core/tests/guard_evaluate.rs`. **Sin cubrir**: `no_verify_is_caught_by_the_second_line`, `aliases_and_plumbing_are_caught_by_the_second_line`, `detected_and_registered_agents_are_the_actor` (solo detectado), `the_message_never_reaches_the_daemon` con espía en el canal (el contrato no tiene campo para el texto: `AuthorshipFacts` es `deny_unknown_fields`), `authorship_entries_in_the_decision_log` (D12). Verificado solo en macOS.

## 11. Enmienda (2026-10-07): segunda línea frente a `--no-verify` y reinstalación 1→2

Implementado en la rama `feat/US-GRD-018-no-verify-second-line`. Cierra el bloqueo técnico de § 8 (leer la línea de órdenes de otro proceso en macOS) y el criterio de reinstalación de D6.

**Decisiones del orquestador (2026-10-07), validadas por el Arquitecto** (`nassa-architect:architect`, una consulta; sus ajustes ya están incorporados). El coordinador aprobó el plan con cuatro ajustes, también incorporados (marcados "coord."):

| # | Decisión | Ajuste de la validación |
|---|---|---|
| S1 | **Crate hermano `crates/macsys`** con el patrón de `winsys`, no un crate de sistema común ([ADR-GRP-002, Enmienda 2026-10-07](../../../../architecture/decisions/ADR-GRP-002-monorepo-nx.md#enmienda-2026-10-07-cratesmacsys)). `process_args(pid)` con `sysctl(KERN_PROCARGS2)`: solo `argc` y `argv`, búfer acotado; cualquier error es `None` | **Arquitecto (bloqueante)**: enmendar ADR-GRP-002 y una sola lista de excepciones en el test de frontera. **Coord.**: mismos lints que `winsys` y el test falla si aparece `unsafe` fuera de `ffi_*` |
| S2 | **Linux** `/proc/<pid>/cmdline` sin `unsafe`; **Windows** `None` | **Arquitecto**: declarar en § 8 que en Windows cada paso de un rebase se evalúa |
| S3 | **El cliente del hook detecta y lee el commit** en `reference-transaction` `prepared` (plantilla ≥ 2): una única pareja `viejo → nuevo` entre `refs/heads/*` y `HEAD`; `nuevo` es un commit cuyo primer padre es `viejo`, cuyos padres son los de `viejo` (amend) o raíz en una ref nueva, y **que ninguna otra ref alcanza** (un fast-forward o un reset a un commit existente no es un commit nuevo). Un `viejo` cero (`update-ref <ref> <nuevo>` sin valor esperado) se toma del valor actual de la ref. El mensaje se lee con gix aislado (≤ 64 KiB, si no, ilegible) y solo viajan los `AuthorshipFacts` (D7) | Sin ajuste |
| S4 | **Contrato**: etapa `CommitStage::SecondLine` detrás de la capacidad nueva `guard.authorship.second-line` (un daemon de PR-A rechazaría la variante con `INVALID_PARAMS`). Sin ella el cliente no la envía y todo sigue como en PR-A | Sin ajuste |
| S5 | **El daemon decide si la evalúa**: sube desde el cliente hasta el `git` antecesor más cercano con el mismo recorrido del solicitante (`requester::parent`, identidad `(pid, inicio)` en cada paso) y lee su línea de órdenes **solo para clasificar** (`crates/policy/src/authorship/subcommand.rs`); no se guarda, no se registra y no se envía | **Arquitecto (bloqueante)**: el mismo recorrido, no un segundo; comprobar `(pid, inicio)` antes y después de leer. **Coord.**: lo desconocido se evalúa |
| S6 | **Una decisión por operación**: el daemon recuerda en memoria (256 entradas, FIFO) los `(pid, inicio)` de los `git` que ya tuvieron decisión en `commit-msg`; su segunda línea se acepta sin reglas ni avisos, así que un `warn` sale una vez. Si la entrada ya no está (expulsada, daemon reiniciado), se evalúa otra vez: lo único que se pierde es que el aviso pueda salir dos veces | **Coord.**: el `git` se identifica por pid más hora de inicio; si no se prueba, se evalúa |
| S7 | **Fail-closed**: sin `git` antecesor, con la hora de inicio cambiada, con la línea de órdenes ilegible (Windows, permisos, proceso terminado), con una opción global desconocida, un alias o `commit-tree` → se evalúa. Solo `rebase`, `cherry-pick`, `revert` y `am` leídos con certeza se saltan. En modo degradado, sin segunda línea (el actor es "sin atribuir", D3) | **Coord.** |
| S8 | **Reinstalación**: `raptor guard install` sobre una instalación confirmada de una plantilla anterior (su `dispatch.conf` dice una plantilla menor o falta un dispatcher) no es "ya instalado": reemplaza en el sitio, archivo por archivo (temporal, `fsync`, `rename` atómico), dentro de la carpeta que registra el diario; nunca deja el repo sin un dispatcher que funcione y no toca la clave. El diario lista los archivos nuevos antes de escribir; una actualización interrumpida deja una mezcla que funciona y la siguiente `raptor guard install` la completa | Sin ajuste. **Nota (2026-10-09, TD-GRD-001)**: la actualización en el sitio cambia en lo que dice "el diario lista los archivos nuevos antes de escribir". Ahora el diario guarda la actualización como **pendiente** (plantilla destino y sus hashes), conserva los hashes confirmados hasta el último paso y la salud acepta ambos; hay puntos de corte por paso y `dispatch.conf` se escribe al final, tras releer cada ejecutable. Ver el [brief de TD-GRD-001](../../../../dev-briefs/td-grd-001-dispatcher-template-3.md) (§ 4.6) y ADR-GRD-001, Enmienda 2026-10-09 |
| S9 | **NFR-01 frente a una denegación en `prepared`**: Git ya escribió el objeto del commit (y, con `-a`, el índice nuevo) antes de la transacción; al fallar, el índice, el árbol de trabajo y la rama quedan como antes, el mensaje sigue en `COMMIT_EDITMSG` y el commit queda como objeto inalcanzable, recuperable | **Coord.**: test e2e |
| S10 | **`voluntary-skips`**: `--no-verify` sigue en la lista (también salta `pre-push` y el mínimo), pero el texto ya no dice que salte la política de autoría; nombra los dos residuos (varios commits con `update-ref`; `commit-tree` y luego `cherry-pick`) | **Arquitecto**: declarar el residuo de `commit-tree` + `cherry-pick` |

**Ronda de revisión** (revisor de código con contexto limpio; los cuatro hallazgos altos y dos bajos, corregidos):

- Un `viejo` cero en `HEAD` y en su rama ya no da dos movimientos distintos: se agrupan por el valor nuevo. Con una ref nueva (o sin valor esperado), **cualquier** commit que ninguna rama alcance todavía es nuevo (`commit-tree` + `branch x <nuevo>`, `update-ref HEAD <nuevo>`).
- Solo las ramas (`refs/heads/*`) y las ramas remotas (`refs/remotes/*`) prueban que un commit "ya existía": un commit aparcado bajo una etiqueta u otra ref (que el agente escribe sin evaluación) y luego alcanzado por fast-forward se evalúa.
- La clasificación exige que `argv[0]` sea `git` o `git.exe`: un `argv[0]` vacío o raro podría desplazar el resto.
- "Una decisión por operación" recuerda, además del `git`, los hechos que vio `commit-msg`, y solo las decisiones `allow`: un `commit-msg` lanzado a mano (un editor que llama al hook con otro mensaje) no hace saltar el commit que llega.
- `replace_files` reutiliza el descriptor de la carpeta ya comprobada; `outdated` mira también la plantilla del diario.

**Residuos y límites declarados** (además de los de § 5.3 y § 8):

- Un agente que aparca un commit nuevo bajo `refs/remotes/*` escrito a mano (`update-ref`) y luego hace fast-forward escapa a la segunda línea.
- Fail-closed con coste: un `git fetch <remoto> x:x` de un agente que trae **un** commit directamente a una rama, o una rama nueva sobre un commit que solo alcanza una etiqueta, se evalúan como commits del agente.
- Rendimiento: cada transacción `prepared` con la forma de un commit (también los de la persona y cada paso de un rebase) lee las ramas y hace una ida y vuelta al daemon. Sin medir en repos grandes; seguimiento.

**Verificación**:

| Criterio | Test |
|---|---|
| D6 · `--no-verify` de un agente con `human-author` + `deny` → bloqueado por la segunda línea; nada se pierde (índice, árbol, rama, `COMMIT_EDITMSG`, objeto inalcanzable); la línea de órdenes no llega al perfil; la persona pasa; con `agents-commit`, sin trailer no pasa y con trailer sí | `apps/cli/tests/guard_us_grd_018.rs::no_verify_is_caught_by_the_second_line` |
| D6 · un alias (`git ci`), `-c alias.x=commit` y `commit-tree` + `update-ref` | `aliases_and_plumbing_are_caught_by_the_second_line` |
| ADR-GRD-003 § 6 · una decisión por operación (el aviso sale una vez, con hooks y con `--no-verify`) | `one_decision_per_commit` |
| D9 · rebase y cherry-pick del agente siguen sin evaluarse (ahora también en la segunda línea) | `amend_merge_rebase_and_cherry_pick` |
| D6 · plantilla 1 sigue igual (sin segunda línea); `raptor guard install` la sube a la 2 y entonces se evalúa, también con `--no-verify`; el mínimo sigue | `a_template_1_install_keeps_working`, `a_template_1_install_is_upgraded_by_reinstalling` |
| S3 · forma del commit (commit, merge, amend, raíz; no: fast-forward, varios commits, sin cambio, rama sobre un commit existente; mensaje mayor que el límite) | `crates/git/tests/commit_shape.rs` |
| S5, S7 · clasificación (opciones globales, `--opt=valor`, alias, opción desconocida, vacía, no UTF-8) | `crates/policy` `authorship::subcommand::tests::*` |
| S5–S7 · `git` más cercano por identidad, pid reutilizado, línea de órdenes ilegible o con la hora cambiada, memoria acotada | `crates/core` `guardrails::second_line::tests::*` |
| S1 · `KERN_PROCARGS2` (este proceso, pid inexistente, áreas mal formadas, nunca el entorno) | `crates/macsys` `process::tests::*` |
| S1 · frontera de `unsafe` con dos excepciones | `crates/winsys/tests/unsafe_boundary.rs` |

**Sin cubrir**: un test de que un daemon con `guard.authorship` pero sin `guard.authorship.second-line` no recibe la etapa (`legacy_protocols.rs`; hoy lo garantiza el cliente comprobando las dos capacidades en `hook.rs::second_line`). Verificado solo en macOS; Linux lo cubre el CI de ubuntu (la suite e2e es `cfg(unix)`); Windows no tiene canal (Pendiente: etapa de validación multiplataforma).


## 12. Estado de PR-B (2026-10-07)

Implementado en la rama `feat/US-GRD-019-who-ran-and-whose-name` (D10, § 6.2). La TUI y el Cockpit quedan fuera (historia del Cockpit aparte, § 8).

| Pieza | Dónde |
|---|---|
| Contrato: `GitEventView.authorship`, `DeclaredAuthorship`, `GitIdentity`, `CoAuthor`, `InferredAgent.trailer`, `TrailerCheck`, capacidad `events.authorship` | `crates/api/src/messages.rs`, `crates/api/src/methods/events.rs` |
| Lectura del commit (autor, committer, mensaje acotado a 64 KiB) con el lector aislado de Guardrails | `crates/git/src/reader.rs` (`commit_identity`), `crates/core/src/daemon/authorship.rs` |
| Columna `events.authorship` (migración append-only) | `crates/core/src/profile/{schema,store}.rs` |
| Pista `inferred` contrastada con el trailer al guardar; sin pista con `human-author` | `crates/core/src/daemon/{authorship,sessions,repos}.rs` |
| Sin la capacidad (y siempre para `raptor-mcp`): sin `authorship` ni `trailer` en `events.history` y en el stream | `crates/core/src/channel/{conn,bus}.rs`, `crates/core/src/client.rs` |
| `raptor events` (texto en/es y `--json`) | `apps/cli/src/events.rs`, `apps/cli/i18n/{en,es}/events.txt` |
| Suite | `apps/cli/tests/events_us_grd_019.rs` |

**Decisiones del orquestador (2026-10-07)**, dentro de lo que la DS ya fija (sin consulta nueva al Arquitecto ni al PO, regla de validación proporcional); plan aprobado por el coordinador con cuatro ajustes, incorporados:

1. La autoría declarada se guarda en una columna nueva `events.authorship` (JSON de `DeclaredAuthorship`), solo para `commit` y `merge`. Del mensaje solo se guardan los co-autores (`Co-Authored-By`); el texto, el asunto y los demás trailers nunca se guardan ni viajan (ajuste 1 del coordinador).
2. `InferredAgent.trailer` es opcional en el cable (`Option<TrailerCheck>`): `InferredAgent` es `deny_unknown_fields` y una conexión sin `events.authorship` debe recibir la forma del protocolo 9. Desviación menor de la firma del § 6.2 (`trailer: TrailerCheck`). Un `commit`/`merge` guardado antes del contraste se lee como `unconfirmed`; los demás eventos no llevan `trailer`.
3. La pista se contrasta una sola vez, al guardar el evento: trailer del mismo agente → `confirmed`; ninguno reconocido → `unconfirmed`; de otro agente → la pista no se guarda; política efectiva `human-author` (con `deny` o `warn`) → la pista no se guarda. Cambiar la política después no reescribe eventos.
4. `raptor-mcp` no pide `events.authorship` (el cliente la filtra) y el daemon no se la aplica aunque la pida (perfil MCP). Hoy el stream del MCP no lleva `git.event` y `events.history` no se le ofrece; la forma sin capacidad se prueba en `channel::bus` (ajuste 2).
5. Texto de `raptor events`: "commit de {autor}[ con {agentes}] · {worktree} ({rama})[ · ejecutado por {agente}[ · sin trailer]]", con {worktree} = nombre de la carpeta del worktree y la rama entre paréntesis para no perder el dato de la línea anterior; la pista añade "(confirmado por el trailer)" o "(no confirmado por el trailer)". Claves nuevas: `events.commit_by`, `events.merge_by`, `events.commit_with`, `events.run_by`, `events.no_trailer`, `events.inferred_confirmed`, `events.inferred_unconfirmed`.

**Criterios del § 7 cubiertos por PR-B**: `agent_commit_shows_both` (también: ni el almacén ni `--json` llevan el texto del mensaje), `an_unattributed_commit_does_not_repeat_the_author`, `the_inferred_hint_is_checked_against_the_trailer` (`confirmed` y `unconfirmed` de extremo a extremo; `contradicted` en `daemon::authorship::tests`, porque la tabla de identidades tiene hoy un solo agente), `human_author_records_no_hint`, `an_agent_commit_without_trailer_shows_the_difference` (mide la presentación; la política `flexible` de extremo a extremo depende de US-GRD-014, § 8), `an_mcp_connection_gets_no_authorship`, `channel::bus::tests::git_events_carry_authorship_only_with_the_capability`, `events::tests::an_older_event_reads_as_before`. **Sin cubrir**: la fila de ADR-GRP-013 "sobrevive a un reinicio; una corrección cambia el actor y no la autoría; prueba de propiedades con autores y trailers aleatorios" (la autoría vive en la fila append-only y no depende del actor, pero no hay prueba dedicada). Verificado solo en macOS (la suite usa `script` y `raptor-fake-agent` de macOS); Linux y Windows: pendiente de la etapa de validación multiplataforma.

## 13. D12 en el registro (2026-10-07, US-GRD-005)

D12 queda cubierto por [DS-US-GRD-005](./US-GRD-005-registro-de-bloqueos.md):

- Toda denegación de autoría y todo aviso de `human-author` + `warn` dejan una entrada con estos datos: el actor, los tipos de agente de los co-autores, si había un trailer de agente, si el mensaje era ilegible y la política aplicada. No se guardan nombres, correos ni el mensaje.
- Una denegación muestra "autor no disponible: el commit no llegó a crearse".
- `flexible` deja un `notice`. Se prueba en `crates/core/tests/guard_evaluate.rs` porque el suelo confirmado no se puede fijar de extremo a extremo hasta US-GRD-014.
- **Sigue pendiente** la unión por oid para mostrar el autor y el committer de un aviso: el contrato `commit` no lleva el oid del commit nuevo. Mientras, el aviso muestra "autor: ver `raptor events`".

`authorship_entries_in_the_decision_log` queda cubierto por `apps/cli/tests/guard_us_grd_005.rs`.

## Estado de la implementación (2026-10-08)

Implementado en: PR #137, #141, #150 (ajustes en #152 y #153); D12 en el registro con #154.

Estado: implementación parcial. Pendiente:
- `detected_and_registered_agents_are_the_actor`: solo se cubre el agente detectado, no el registrado (DS § 7).
- Nivel local (`settings.local.json`): US-GRP-013. `flexible` de extremo a extremo con un suelo de equipo: US-GRD-014.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../../../../architecture/xplat-pendientes.md)).

Sincronizado con los PR mergeados por la tarea `docs/sync-story-status` (2026-10-08).
