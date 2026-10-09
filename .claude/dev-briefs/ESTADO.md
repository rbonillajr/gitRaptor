# Estado: fix/tui-q-quits-hang (2026-10-09)

## Causa (confirmada)
El test `q_quits_and_restores_the_terminal` escribía `q` a los 1500 ms fijos. Si la TUI aún no estaba en modo raw (máquina lenta), la tecla se queda en la disciplina de línea y la TUI nunca sale; `wait_with_output` esperaba para siempre. Reproducido a mano: `printf q | script -q /dev/null raptor` → cuelga. No se reprodujo en 70 corridas del test viejo (ni con carga), porque depende de la carrera.
No es un fallo del producto: no se tocó `term.rs`.

## Hecho (commit pushed en la rama)
- `apps/cli/tests/tui_process.rs`: `Session` (script + hilo lector) con plazo de 30 s en cada paso, `q` enviado al ver la pantalla alterna, fallo con lo leído, kill del hijo de `script` y de `script` en Drop; `Fixture::drop` mata el daemon con TERM y KILL tras 5 s.
- Los 3 tests pty usan `Session`.
- 20 corridas completas del archivo con carga de CPU: 0 fallos, sin huérfanos propios.

## Falta
1. `cargo clippy --all-targets -- -D warnings` y `cargo test --workspace` completos (corrían en segundo plano al cortar; fmt --check y clippy del test ya en verde).
2. Abrir PR (`gh pr create --base main`), con causa y mecanismo, `gh pr merge <n> --auto --rebase`, sección "Prueba de plugins".
3. Anotar en el PR: había daemons huérfanos de otras ramas (uxmintty, ctrlz, grd004) en la máquina, ajenos a esta tarea.
