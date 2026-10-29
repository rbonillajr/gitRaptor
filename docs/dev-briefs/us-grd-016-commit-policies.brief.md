# Brief · US-GRD-016 · Políticas de commit en el plan de commit del ejecutor (parte A)

Decisión del orquestador (2026-10-28), validada por el Arquitecto (`rust-architect`, diseño y división) y el PO (ELIJO 2).

## Hueco

`safe_commit` por `raptor-mcp` no aplica `policy.commit-format` ni `policy.diff-size` (US-GRD-009). El plan de commit se decide con `RuleSet::all()` (`commit_policies: false`), y los hooks del `git` hijo reutilizan esa decisión. Con Git directo, el hook las deniega. El MCP es, por tanto, más laxo (NFR-02). DS-US-GRD-009 D17 traspasó esto a US-GRD-016. D10 de DS-US-GRD-016 partía de que esas reglas no existían, pero llegaron con el PR #296.

## Alcance de este PR (parte A)

- **`policy.commit-format`, exacto.** El plan lleva la cabecera ya parseada del mensaje y la evalúa con la misma política pura que el hook. El motivo y el nivel son los mismos, hay una sola entrada de registro con capa `mcp` y el rechazo `policy-denied` lleva sus `reasons`.
- **`policy.diff-size`, falla cerrado.** Si una regla de tamaño gobierna al actor, el plan da `unverifiable` (deniega con causa `unverifiable`) y el mensaje indica la vía alternativa: hacer el commit con Git directo, donde el hook decide con exactitud. Es más estricto que Git directo, nunca más laxo. Ninguna plantilla por defecto activa `diff-size`; sin configuración no se exige nada (`guard_us_grd_009::nothing_is_required_without_configuration`).
- Fusión, rebase y worktrees no cambian: `RuleSet::all()` no se toca, y solo el plan de commit activa `commit_policies`.

## Diseño (del Arquitecto, recortado a la parte A)

1. `crates/policy/src/guard/plan.rs`: `PlanShape::Commit` gana `header: Header` (ya parseado). `Preset` gana `commit: Option<PlannedCommit>`, con `PlannedCommit { header, parents, paths }`, que arma `normalize` para `preset[1]`. La puerta nunca recibe el texto del mensaje.
2. `crates/core/src/executor/ops/commit.rs::plan_op`: calcula `header` con una función nueva, `commit_header(common, message)`, junto a `commit_facts`. Aplica la misma limpieza del mensaje (`commit.cleanup`, `core.commentChar`) y después `message_facts` y `parse_with`. Hay que hacer `pub` `message_facts` en `crates/git/src/guard_changes.rs`. `parents` vale 0 si `old` es `ZERO_OID` y 1 en otro caso.
3. `crates/core/src/guardrails/commit_policies.rs`: `pub fn prospective(...) -> Vec<Option<NewCommits>>`, con las mismas compuertas que `changes()`. Si `needs_commit_facts(actor)` es falso, devuelve `vec![]`. Si una regla de líneas o de ficheros gobierna al actor, devuelve `NewCommits { unverifiable: true }` (parte A). El formato no hace E/S: `CommitFacts { oid: "", header, parents, .. }`.
4. `crates/core/src/guardrails/evaluate.rs::evaluate_preset`: con `preset.commit` en `Some`, usa `prospective(...)` y nunca `changes()` sobre el centinela `old→old`.
5. `crates/core/src/guardrails/plan_gate.rs::decide`: solo para `PlanShape::Commit`, `rules: RuleSet { commit_policies: true, ..RuleSet::all() }`.
6. `crates/policy/src/guard/commit_policies.rs`: con `oid` vacío, no se emite `ParamKind::Oid`. En `apps/cli/src/guard.rs::commit_policy_text`, si falta el oid se usa la clave nueva `guard.reason.new-commit` («el nuevo commit» / «the new commit»).
7. Los tests existentes que construyen `PlanShape::Commit` ganan el campo (`us_mcp_009_permission.rs`, `us_grd_016_parity.rs`, `plan_gate_tests.rs`, `plan_commit_tests.rs`, `plan_tests.rs`).

## Pruebas

- `crates/core/tests/us_grd_016_commit_policies.rs` (nuevo): repo temporal con `HOME` y configuración aislados. Compara el plan con capa `mcp` frente al `reference-transaction` real del hook. Filas:
  - mensaje fuera de formato: misma `(rule, level, cause, effect)`, `params` sin `Oid`;
  - mensaje conforme: `allow`;
  - persona no gobernada: `allow`;
  - `diff-size` que gobierna al agente: el plan deniega con `unverifiable` aunque el hook lo permitiría (falla cerrado, más estricto).
- `apps/cli/tests/guard_us_grd_016_commit_policies.rs` (nuevo, solo macOS con `debug_assertions`): `safe_commit` con un mensaje fuera de formato da `policy-denied` con `policy.commit-format` en `rules` y `reasons`. La rama y el índice quedan intactos y hay una sola entrada de registro con `layer: mcp`. Es el escenario MCP de US-GRD-009 (L101-110).

## Fuera (parte B, pendiente en US-GRD-016)

- `diff-size` exacto. Reconstruye en memoria el árbol futuro (índice o worktree según `from_index`) y lo cuenta con `lines_between` y detección de renombrados, como el hook (`guard_prospective.rs`, unas 400 líneas, riesgo medio). Pide revisión de seguridad de las rutas del worktree.
- Comprobar en el hijo el commit real frente a lo previsto (carrera entre la decisión y el `commit`), y los hooks `commit-msg` del usuario que reescriben el mensaje.
