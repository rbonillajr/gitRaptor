# Preparación de la corrida · US-CKP-022 (2026-11-10)

Sin diseño: `docs/requirements/features/cockpit/dev-specs/US-CKP-022-grafo-carriles.md` ya es el plan (`spec-ready`). El Arquitecto (opus) la revalidó contra `main` (`905c66b5`, tras US-CKP-012 #360) y registró D14 a D20; el loop principal prepara el contrato. Modo rápido (2026-11-08): sin línea base de suite completa; se construye SIN CERTIFICAR y la suite la corre el CI.

## Deriva de citas
Ya medida por el Arquitecto: las citas de la DS están actualizadas a `main`. Cambios que importan al implementador: `.since(CAPABILITIES_PROTOCOL)` (D14), enum propio `GraphFailure` (D15), `GitLaneGap` en `crates/git` (D16), caché como campo de `ServerCtx` (D17), plazo de prueba con `test_diff_deadline` (D18), `observe::open_worktree` para abrir cada worktree y regla de repo de `diff.rs` (D19), vista de grafo y de diff mutuamente excluyentes (D20).

## Ajustes a la DS para el contrato (no cambian ninguna decisión)
- **Pruebas unitarias en archivos propios** (la guarda R4 congela los de prueba): `apps/cli/src/tui/graph_tests.rs`, `present/graph_tests.rs`, `present/graph_text_tests.rs`, `client/tests/graph.rs` y `tui/view/graph_tests.rs`. Las líneas `#[cfg(test)] #[path = "…"] mod …;` son producción: las añade el experto. Hasta entonces no compilan y el `verify` (que comprueba con `-- --list` que el test existe) sale en rojo. Las pruebas de API, git, core y e2e son integración y compilan contra tipos del contrato que el experto crea (`crates/api/src/graph.rs`, `crates/git/src/lane.rs`).
- **Pruebas añadidas a § 9.4** para comportamientos que la DS define sin test propio: `a_stop_mid_walk_is_too_slow_never_a_partial_window`, `missing_base_is_not_computable_without_lanes`, `a_second_request_on_the_connection_is_query_busy`, `unknown_repo_is_not_found`, `g_is_inert_without_a_repo_the_capability_or_with_the_diff_open`, `a_graph_request_is_answered_by_ticket_and_a_reconnect_loses_it`, `base_not_computable_messages_are_exact` y `graph_snapshots_panel_100x30_and_view_80x24`.
- `graph.lanes` nace con `mcp: false` y sin presupuesto propio (P27 de DS-US-DSK-009): `CKP022-CORE-MCP` lo fija.

## Supuestos (reversibles)
- Un solo experto (`sonnet`): una sola cadena de crates comparte `target/`. Los tramos siguen T001→T006 de la DS.
- Pruebas e2e (`tui_graph.rs`) solo macOS en debug, como `live_fleet.rs`; Linux y Windows quedan pendientes de la etapa de validación multiplataforma.
- Los snapshots (`graph_{panel_100x30,view_80x24}_{en,es,ascii}.snap`) los crea el tester junto a la prueba; si insta no tiene baseline, el experto los acepta con revisión.

## Cambios tras la ronda 1 del crítico (PC-1 a PC-7)
- `lane_totals_match_the_ahead_of_the_stream`: solo el `total` de cada carril coincide con el ↑; `lanes_total` cuenta carriles.
- La caché (`second_refresh_hits_the_lane_cache`) pasa a prueba unitaria `crates/core/src/channel/graph_tests.rs` (el experto añade `#[cfg(test)] #[path = "graph_tests.rs"] mod tests;` en `channel/graph.rs`), con un contador de recorridos solo de pruebas.
- Criterios nuevos: `CKP022-CORE-UNAVAILABLE` (D19) y los cuatro de `client/tests/graph.rs` con los nombres de la DS.
- `lane_total_equals_ahead_behind_commits` (`only_in_walk` es privada al crate) y `GitLaneGap::NoMergeBase`.
- `responses_are_never_logged_nor_persisted` comprueba el log del watcher, el perfil y los logs del motor (sin gitleaks).
- § 9.4 de la DS alineada con el contrato.

## Tras el tester (2026-11-10)
- Renombrados para no chocar con pruebas existentes de diff (sus `verify` salían en verde hoy): `a_graph_reply_for_an_old_ticket_is_dropped`, `closing_the_graph_while_loading_cancels`, `every_graph_text_has_both_languages`.
- E2E-5: con 12 filas y el panel de conflictos, `layout` aún deja ~7 filas al grafo; el escenario comprueba lista, pista `g` y apertura de la vista, y la cesión ante el panel de conflictos la prueba `CKP022-CLI-YIELD`. DS § 9.4 alineada.
- API de `LaneCache` fijada en la DS (T003) según `channel/graph_tests.rs`. Los tests de `apps/cli` asumen `graph::scroll(model, down: bool)`, `Anchor::Lane(u64)`, `GraphText::every()`, `Base::Missing(SafeText)` y `ConnEvent::Graph(bool)`: el experto los respeta (T005/T006 de la DS).
- Los 6 `.snap` los crea y revisa el experto. Montajes de `mod` de prueba: `tui/mod.rs`, `present/mod.rs`, `view.rs`, `client/mod.rs` (`tests::graph`) y `channel/graph.rs`.
