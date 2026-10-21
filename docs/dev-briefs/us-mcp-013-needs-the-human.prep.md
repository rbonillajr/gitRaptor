# Preparación de la corrida · US-MCP-013 (2026-10-20)

Sin diseño: `docs/requirements/features/mcp/dev-specs/US-MCP-013-dev-spec.md` ya es el plan (`spec-ready`). La preparó el loop principal. Esta nota solo recoge lo que la preparación encontró contra el código de hoy.

## Deriva de citas del spec (ajustes que no cambian ninguna decisión)

1. **T002 paso 4**: el spec dice "una sola llamada a `self.gate.evaluate(&req)`". El código de `run` (`crates/core/src/executor/mod.rs`, "The decision that counts") llama a `self.gate.evaluate_detailed(&req)` y usa `verdict.view`, `verdict.decision` y `verdict.binding`; `evaluate_detailed` por defecto llama una vez a `evaluate`. La regla de D2/⛔2.1 se conserva: **una sola** llamada a `evaluate_detailed` en `run` y una `match` sobre `verdict.view`. `Allow` sigue con `verdict.binding`; `Ask` cierra con `PlanClose::AskedWithoutQueue` y devuelve `ExecError::Rejected(RejectReason::GuardrailsConfirmationRequired)`; `Deny | NotEvaluated` sigue como hoy (`PlanClose::Denied`, `ExecError::GuardrailsDenied(denial)`).
2. **T002 paso 3** (vista previa de `prepare`): la `match` está sobre `verdict.view` tras `evaluate_detailed`; el brazo nuevo `GateDecision::Ask => DecisionView::Deny`. El `denial` de la vista previa sigue filtrado a `Deny`.
3. El rechazo de `run` no pasa por `refuse()` (que cierra con `Rejected`): se cierra una vez con `AskedWithoutQueue` y se devuelve el error directo.

## Decisiones de la preparación (reversibles)

- **Guardas de T004 fuera del contrato.** `the_server_is_built_without_elicitation` y `a_session_never_receives_a_request_from_the_server` pasan con el código de hoy (el propio spec lo dice): como criterio, la línea base B3 detendría la corrida. Las escribe el experto en `apps/mcp/tests/no_elicitation.rs` (T004) y corren en la suite completa y en el CI; no son criterios del contrato.
- **Criterios de pruebas de `raptor-mcp` (binario).** Un archivo `#[path]` sin su línea `mod` no se compila, y `cargo test <filtro>` sin coincidencias sale 0. El `verify` de esos criterios primero comprueba con `-- --list` que la prueba existe y luego la corre con `--exact`. Las líneas `#[cfg(test)] #[path = …] mod …;` en `messages.rs` y `engine.rs` son producción: las añade el experto.
- **Criterios estáticos de documentos** (T005/T006): un `grep` por ADR/contrato clave. El resto de T005 se revisa en el review y en `docs-lint`.
- **Un solo experto**: `brief-slices` no puede leer el árbol de § Estructura de ficheros (está en un bloque de código); los tramos A→B/C comparten `target/` de todos modos.
- **Coordinación con W79** (US-GRD-007): produce el `ask` en la decisión de Guardrails; esta entrega lo consume en el ejecutor y en el MCP. El segundo en entrar integra si se tocan los mismos archivos; `crates/policy/**` y `crates/core/src/guardrails/**` quedan en `deny_paths`.
