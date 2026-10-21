# Evidencia — US-CKP-005 (la TUI pequeña, sin color y en ASCII)

Corrida de `/nassa-core:implement --autonomous`, rama `feat/US-CKP-005-small-terminal`, 2026-10-21.

- **Plan:** [DS-US-CKP-005](../requirements/features/cockpit/dev-specs/US-CKP-005-terminal-pequena-sin-color.md) (preparada en el loop principal; el crítico opus, 1 ronda, APROBABLE).
- **Contrato:** `us-ckp-005-small-terminal.contract.json` · `contract_sha256: 588089e802c8840786b9cadef9ce4c802d230661a86426eca13e15d2dcc1f355`.
- **Certificación:** `us-ckp-005-small-terminal.certification.json`: **CERTIFIED WITH RESERVATIONS**, 11 de 11 criterios.

## Reservas

- `baseline-roja`: la suite base salió roja por 9 tests ajenos del daemon («the GitRaptor daemon did not start in time», bajo carga de otros workers). Aislados pasan `raw_git_undo`, `tm_chaos` y `guard_us_grd_001`; `guard_td_grd_001_upgrade` pasó 8 de 9 y falló una vez más por el arranque del daemon. La persona admitió la base roja (turno comprobado).
- `integridad-sin-verificar`: P3 no registró informe de nextest (ver «Prueba de plugins» del PR).
- `test-enmendado` ×2: `tui_screens.rs` y `tui_accessibility.rs`, aprobadas por la persona (ver diff en la certificación).

## Revisión (veredictos tal cual)

- `lens:correctness` (sonnet): APPROVED WITH COMMENTS. Un Low: `Say` pliega también los datos que el catálogo intercala (un `–` en una ruta se ve `-` en ASCII). Es la nota G3 de la Dev Spec.
- `lens:testing` (sonnet): APPROVED WITH COMMENTS. M1: las dos enmiendas de `tui_screens.rs` debilitan asertos (aprobadas; cobertura recuperada en `the_ascii_fold…` y en la ayuda). M2: `settle()` espera 400 ms de silencio, una cota y no una señal; puede ser intermitente bajo carga. L1: sin guarda de ratón en `tui_boundaries.rs` (añadida en `tui_no_mouse.rs`).
- Seguridad (`security-expert`, sonnet): PASS, sin Critical, High ni Medium. Info: `Say::text` devuelve `String` (I-01) y un `&str` crudo de `worktree.name` entra en `Text` (I-02).
- Conformidad (walker `rust-code-reviewer`, sonnet): todos los escenarios al escalón 2 como mínimo, `?`/Esc y `--ascii` al 3 (pty). Faltaban tres casos pty de T001; se añadieron en `tui_accessibility_env.rs` (verdes). `nine_agents_are_told_apart_by_name` y `tui_no_mouse.rs` quedan como «no pedido» (guardas de regresión).
