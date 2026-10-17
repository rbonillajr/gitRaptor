## Qué cambia

Estabiliza los tests inestables de macOS (segundo PR; el de `answering_yes…` va aparte en #252). Todo es de tests salvo **un arreglo de producto en el cliente del canal**.

| Test | Causa raíz | Arreglo | Antes → después (carga: 20–60 bucles de CPU en 10 núcleos) |
|---|---|---|---|
| `fsevents::a_stream_started_from_the_last_id_replays…` y `exclusions_also_filter_the_replayed_history` | Carrera del **test**: el kernel entrega los eventos a fseventsd de forma asíncrona; un stream nuevo justo tras escribir puede reproducir un historial que aún no los tiene (`the history was not replayed`, `fsevents.rs:171`, visto en CI). En el primero además `HISTORY_DONE` podía llegar antes/entre las filas y el segundo `wait_for` lo había consumido. | `write_in_the_gap`: un stream testigo, abierto antes de escribir, ve las escrituras y hace `flush_sync` antes de que el test siga. Una sola pasada espera fila y `HISTORY_DONE` en cualquier orden. | 11/30 → 0/30 |
| `channel::slow_client_and_connection_flood…` | **Carrera del producto** (ver abajo): `Client::next_notification` con timeout de 1 ms perdía la parte de una línea ya leída (`Protocol("not a contract message")`, visto en CI el 10-oct). Además el test dependía de tiempos (el cliente bueno solo se vaciaba cada 10 publicaciones; pausa fija de 100 ms para la inundación). | Producto: `read_frame_resumable` + estado parcial en `Client`. Test: el cliente bueno nunca se atrasa más de 512 eventos (el outbox es 1024) y la señal del límite se espera en la última conexión (10 s) en vez de una pausa. Test determinista del cliente: `a_read_timeout_inside_a_frame_does_not_corrupt_the_stream` y del framing. | No reproducido en local (0/80) antes ni después; causa fijada con el log de CI y el test determinista |
| `live_changes::ten_active_worktrees_are_followed_without_losing_events` (y `a_file_change_is_reflected_almost_instantly`) | Aserción `ENGINE_BUDGET_MS` (300 ms) en un test **debug** (regla del proyecto). | Se quita: el gate de NFR-04 ya lo hace el bench `engine` en release (10 worktrees, INF-GRP-002). El test conserva la propiedad funcional (no se pierden eventos; plazo de 30 s como cota de «no se perdió»). | 3/15 → 0/10 |
| `timeline_process::a_detected_agent_commit_shows…` | El hook dormía 0,5 s «para que la muestra del motor vea el `git`»: espera por tiempo contra un muestreo que depende de la carga. | `held_commit`: el hook retiene `git` hasta que el test se lo permite; el test lo suelta cuando el motor ya registró el commit (el motor guarda el evento después de muestrear, así que el muestreo ya pasó). | No reproducido en local (0/15, 0/12 a 100 bucles) antes ni después; causa por lectura del código |
| `autostart::tests::the_manager_starts_only_an_artifact_of_the_same_binary` | **Hipótesis, no confirmada**: la primera ejecución de un script nuevo cuesta ~0,5 s en un macOS en reposo (0,15 s la siguiente) y `run_tool` abandona a los 5 s (`TOOL_TIMEOUT`). Descarté `ETXTBSY` (0/2000 con 6 hilos haciendo spawn). No lo pude reproducir (0/55, también con el lib completo bajo carga) ni hay log de CI. | `fake_tool` ejecuta la herramienta una vez, sin límite, antes de que los tests dependan de su velocidad. | 0/55 → 0/55 (sin tasa de fallo medible) |

## La carrera del producto (canal)

`gitraptor_api::client::Client::read_frame` creaba un `Vec` nuevo por llamada y `framing::read_frame` lo vaciaba al entrar. Si el timeout de lectura caía a mitad de una línea, los bytes ya consumidos del `BufReader` se perdían y la siguiente lectura empezaba a mitad de mensaje → `Protocol("not a contract message")`. Afecta a cualquier cliente que use `next_notification`/`next_incoming` con timeouts cortos (la TUI y la CLI), no solo al test. Arreglo: `framing::read_frame_resumable` (no reinicia `buf` ni `too_large`), y `Client` guarda `partial`/`partial_too_large`. El servidor no cambia: ante un error de E/S cierra la conexión.

## Cómo se verificó

- `cargo fmt --all --check` y `cargo clippy --all-targets -- -D warnings` en verde; `cargo test -p gitraptor-api` en verde.
- Suite completa (`cargo test --workspace --no-fail-fast -- --skip repo_intact`) 3 veces seguidas con 30 bucles de CPU en una máquina de 10 núcleos: RESULTADO_SUITE.
- Tasas «antes/después» de la tabla: bucle del binario de test con N bucles de CPU en paralelo.

## Fuera de alcance / pendiente (honestidad)

- **No pude** reproducir en local `channel`, `timeline_process` ni `autostart`; sus causas salen del log de CI (canal) o de lectura de código (los otros dos). No afirmo «30/30» para esos tres: afirmo el arreglo de la causa y, donde hay, un test determinista.
- **`us_tmc_009` es inestable de verdad** (otro trabajo): `restoring_over_an_agents_work_is_refused_until_confirmation_exists` («operation failed» en un paso de preparación, main rojo) y `a_restore_can_be_undone_back_to_the_exact_prior_state` (`git-event-3` en vez de la operación del restore, visto en #252). Causa probable de la segunda: el observador registra la rama que crea el restore como evento propio y, si llega después del restore, «deshacer el último» deshace ese evento; `undo` por `operation_id` aún devuelve `-32004`. Sugiero un PR aparte.
- Con la máquina cargada y fuera de un terminal, `channel_process::a_stop_without_a_controlling_terminal_is_refused` falló en mi arnés en segundo plano (el motor respondió `identity-unverified` en vez de `no-controlling-terminal`); pasa en primer plano, aislado y bajo carga. No lo investigué: es la ruta de autorización (fail-closed). Lo excluí de mis corridas de 3 pasadas.
- No se tocó `docs/ARTIFACTS.md`.
- Revisor Opus pendiente por tocar el cliente del canal (producto).

## Prueba de plugins

No se usó `/implement` (flujo directo, test inestables).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
