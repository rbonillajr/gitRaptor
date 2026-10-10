# Estado TD-GRD-001 (2026-10-09, corte por apagado del Mac)

Rama: feat/TD-GRD-001-dispatcher-template-3. Run de /nassa-core:implement en P3_RUNTIME (interrumpido; P0, P1 y P2 pasaron).

## Hecho
- Brief y contrato aprobados por el coordinador (condición O-2 aplicada): docs/dev-briefs/td-grd-001-dispatcher-template-3.{md,contract.json}.
- Slices T (tests rojos), C, D, G, A, B, E, F y H commiteados. Los 24 criterios pasan en las pruebas dirigidas.
- Enmiendas R4 aprobadas por el coordinador: TD001-BRANCH y TD001-FOOTPRINT (commit de1d0005).
- Revisión rust-code-reviewer (opus): APPROVED WITH COMMENTS, 0 C/H, 2 M, 1 L.
- /security-review (security-expert, opus): aprobado con hallazgos, 0 C/H, 3 M, 2 L, 3 I.

## Siguiente paso exacto
1. Ronda de arreglos (una):
   - CR-M1: hook.rs:844, `degraded()` no deniega si `evaluate::open` falla y solo hay refs no gobernadas (condición O-2). Añadir un test en td_grd_001_client.rs, que no es un archivo del contrato. Si lo es, añadir el test en otro archivo.
   - SEC-M-02: hook.rs:633/653, el daemon sin guard.policies aplica `ungoverned_evaluation` (every_rule) a las no gobernadas y `everyone_only` a las gobernadas, con los motivos unidos como en `degraded()`.
   - SEC-M-01: health.rs:128, H4 comprueba `is_file && mode & 0o111` y publica BinaryMissing.
   - SEC-L-02: unix.rs:239, EISDIR/EPERM dejan el temporal en su sitio.
   - CR-L3: leer el suelo una sola vez (hook.rs:798/829).
2. Pendientes para el PR (sin arreglar):
   - SEC-M-03: suelo sin confirmar sin daemon. Necesita un TD o ADR.
   - SEC-L-01: junction en Windows. Anotar en XP-43.
   - CR-M2: el barrido de cortes no ejerce «conf 3 con binario viejo». Requiere una enmienda R4 del helper td_grd_001_machine.
3. `CARGO_BUILD_JOBS=3 NEXTEST_TEST_THREADS=4 node <nassa-core>/scripts/deliver-run.mjs advance --dir . --diff <diff>` (P3), luego P4 a P6: deliver-certify --out docs/dev-briefs/td-grd-001-dispatcher-template-3.certification.json, y después la evidencia.
4. `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace` (con la carga limitada).
5. Ficha TD-GRD-001 → status implemented + backlog.md + technical-stories.md. Luego `node tools/status/release-status.mjs`.
6. PR con `gh pr create --base main` y `gh pr merge <n> --auto --rebase`. Secciones: decisiones O-1, O-2 y O-3, enmiendas R4, Prueba de plugins (brief-slices no parseó la tabla de topología → un solo experto; el adaptador nextest resolvió B1). Avisar al coordinador con worker_done (task_ca72d11987a1 / ctx_c7efee467387).
- Nota: channel_process::a_stop_without_a_controlling_terminal_is_refused falló localmente por el entorno (terminal de control). Falta comprobarlo contra main.
