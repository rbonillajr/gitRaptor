# Evidencia · TS-CKP-003, capa del Cockpit en la decisión de Guardrails

- **Plan**: [DS-TS-CKP-003](../requirements/features/cockpit/dev-specs/TS-CKP-003-capa-cockpit-guardrails.md) (preparado en el loop principal, sin rediseño).
- **Contrato**: [ts-ckp-003-cockpit-guardrails-layer.contract.json](ts-ckp-003-cockpit-guardrails-layer.contract.json), 23 criterios. `contract_sha256` certificado: `7499b04eed01cd681ab672c433ce77f381c5b2875eec69b620679394f36fa784`.
- **Aprobación**: el 2026-10-17, por la persona (menú) y por el coordinador, con la condición de que los brazos de prueba existan solo en debug (criterio CKP003-RELEASE).
- **Certificación**: [ts-ckp-003-cockpit-guardrails-layer.certification.json](ts-ckp-003-cockpit-guardrails-layer.certification.json).

## Certificación

**23/23 criterios PASS**, y aun así el veredicto es **NOT CERTIFIED (`bloqueada`)** por **X6**: «el contrato no es el que firmó la aprobación».

X6 sale de las dos enmiendas de pruebas que aprobó el coordinador (`deliver-contract amend-test`). La enmienda añade `amendments` a la copia medida del contrato y le cambia el sha; el contrato aprobado (`docs/dev-briefs/…contract.json`) no cambió desde la aprobación. Las dos enmiendas sí aparecen bien como reserva `test-enmendado`, con su diff:

| Criterio | Prueba | Cambio | Motivo |
|---|---|---|---|
| CKP003-LOCK (y D15) | `crates/core/tests/ts_ckp_003_deadlock.rs` | `root: tp.root.path().join("profile")` | El cliente del hook recibía la raíz del perfil sin `/profile` y nunca conectaba |
| CKP003-CHAIN | `apps/cli/tests/guard_ts_ckp_003_chaining.rs` | se excluye `PriorHooks::Global` | El daemon de test lee la configuración global de la cuenta (precedente `guard_us_grd_003.rs` E4); lo cubren `guardrails::prior` y `crates/git/tests/user_ops_hooks_env.rs` |

Otras reservas:
- `baseline-roja-explicada`: la línea base estaba en rojo solo por las 50 pruebas del contrato.
- `integridad-sin-verificar`: P3 corrió `cargo nextest run` y no `suite.command`, así que no se registró el informe por archivo.

Aviso `fuera-del-plan`: 21 ficheros. Todos están justificados por la conformidad: cableado del daemon, brazos de prueba, `user_ops` por la enmienda de ADR-CKP-002, el adaptador de JUnit y las firmas que cambian.

## Verificación

- `cargo test --workspace` completo tras el rebase sobre `main` (con #249 y #260): **2234 passed, 0 failed** (macOS).
- `cargo clippy --workspace --all-targets -- -D warnings` y `cargo fmt --all --check`: limpios.
- CKP003-RELEASE corre `cargo test --release` sin `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`.
- Línea base: tres mediciones salieron con un test inestable distinto cada una bajo la carga de otros workers (`raw_git_undo`, `us_tmc_005_hook` rebase, `mcp_security_corpus`). Los tres pasan aislados y en `origin/main`. La cuarta, con `NEXTEST_RETRIES=2`, salió sana y sin reintentos.

## Revisión (lentes, sin Critical ni High)

| Lente | Modelo | Veredicto | Hallazgos |
|---|---|---|---|
| correctness | sonnet | APPROVED WITH COMMENTS | M1 (petición inválida de un hijo directo sin `late`): **arreglado**. L1 (denegación tardía solo informada si el run falla) y L2 (transición registrada heredable más de una vez): pendientes |
| testing | sonnet | APPROVED WITH COMMENTS | M1 (sin test de release): ya lo cubría `a_release_build_never_arms_the_test_arms`. L1 (umbral de 10 s duplicado en el test de interbloqueo) y L2 (sondeo de socket con plazo de 5 s): pendientes, en pruebas congeladas del contrato |
| architecture | opus | APPROVED WITH COMMENTS | L1 (doc de `gate.rs`): **arreglado**. L2 (el ejecutor instancia el motor concreto) y L3 (`classify` se calcula dos veces por `guard.evaluate`): pendientes |
| security (`/security-review`) | opus | APPROVE | L-01 (plan ejecutado expulsado como «dropped»): **arreglado**. L-02 (`remote.*.promisor` global o de sistema no detectado como *partial clone*): pendiente. I-01 a I-04: informativos. La re-revisión tras el arreglo también es APPROVE |

## Conformidad

Todas las cláusulas A1–A10 de la ficha y los ítems del § 9.4 tienen tarea dueña, y están en escalón 2 o 3, salvo **A7** («omitir el snapshot previo por hook para hijos del ejecutor»), que está en escalón 1. Lo cubre de forma indirecta `the_executor_child_never_asks_for_a_hook_prior`. Nada sobra «no pedido».

## Sin verificar

- Linux y Windows: *Pendiente: etapa de validación multiplataforma* (XP-18 y XP-19). Las E2E son `cfg` macOS.
- Latencia p95 contra NFR-GRD-04: no se midió en esta entrega.
