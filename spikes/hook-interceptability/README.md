# SPIKE-GRD-001 — Prototipo de interceptabilidad de la capa de hooks

Prototipo aislado del [SPIKE-GRD-001](../../docs/requirements/features/guardrails/technical-stories/SPIKE-GRD-001-interceptabilidad-hooks.md). Los resultados y las conclusiones están en [`SPIKE-GRD-001-resultados.md`](../../docs/requirements/features/guardrails/research/SPIKE-GRD-001-resultados.md).

> **No es código de producto.** Son scripts `bash`/`sh` que crean repos temporales con `mktemp -d`, instalan dispatchers de prueba con `core.hooksPath` y ejecutan Git crudo. **Nunca** tocan este repo ni la configuración global del usuario: cada sandbox redefine `HOME` y fija `GIT_CONFIG_NOSYSTEM=1`, y `sandbox_guard` aborta si un comando de prueba se ejecutaría fuera del sandbox (NFR-01).

## Qué hay

| Ruta | Qué es |
|---|---|
| `lib/common.sh` | Sandbox, repos de prueba, instalación y desinstalación de dispatchers (`install_probes`, `install_guard`, `uninstall_guard`) |
| `lib/probe-hook.sh` | Dispatcher **sonda**: registra cada invocación (hook, argumentos, entrada estándar, cwd, `GIT_DIR`) y la huella de refs, índice, working tree y estado en ese momento. Con `PROBE_DENY` sale con 1 |
| `lib/guard-hook.sh` | Prototipo **guard** en `sh` de la decisión de US-GRD-001: deniega el force-push y el borrado de la rama base. Solo constantes, sin daemon y sin NFC |
| `lib/fp.sh` | Huellas leídas del disco, sin ejecutar hooks |
| `lib/hookstub.rs` | Binario nativo que sustituye a `raptor hook` en las mediciones de coste (se compila con `rustc -O` dentro del sandbox; no es parte del workspace de Cargo) |
| `lib/bench.py` | Cronómetro (p50 y p95) de un comando |
| `suites/01-matrix.sh` | Matriz de operaciones de BR-VAL-002: qué hooks corren, en qué orden y en qué momento (A, B o C), y si denegarlos deja efectos |
| `suites/02-forcepush-basedelete.sh` | Force-push y borrado de la rama base con el guard (US-GRD-001), incluidos los alias, los grafts, los worktrees enlazados y los saltos |
| `suites/03-managers.sh` | Coexistencia con hooks propios, husky, lefthook y pre-commit: encadenado, reinstalación del gestor y desinstalación |
| `suites/04-config-worktrees.sh` | Huella byte a byte del `config`, cobertura de worktrees (`config.worktree`, `include`, `includeIf`) y `GIT_DIR` cruzado |
| `suites/05-hookset.sh` | Hooks que cambian el comportamiento de Git por existir, y número de invocaciones por comando |
| `suites/06-cost.sh` | Coste por invocación: sin hooks, dispatcher `sh` con salida rápida + binario nativo, solo binario, guard en `sh`; fetch de 1.000 refs |
| `suites/07-cost-native.sh` | Coste con el binario nativo como dispatcher (variante VN), portable a Windows: cronómetro en Rust (`lib/bench.rs`), sin Python; comprueba que Git ejecuta un hook nativo con y sin `.exe` (2026-10-05, US-GRD-001) |
| `lib/bench.rs` | Cronómetro (p50 y p95) de un comando lanzado sin shell, para la suite 07 |
| `results/<so>-<arch>-git<versión>[-reftable]/` | Evidencia de cada ejecución (TSV); `01-matrix-detail/` guarda la traza de hooks de cada caso |

## Cómo reproducir

Requisitos: `bash`, `git`, `python3`, `rustc` (solo para la suite 06), y `node`/`npm` (solo para la suite 03).

```sh
cd spikes/hook-interceptability
./setup-tools.sh                          # husky, lefthook y pre-commit en ./.tools (ignorado por Git)
SKIP_COST=1 ./run-all.sh                  # suites 01–05 con el git del PATH
GIT_BIN_DIR=/ruta/a/git-2.38.5/bin SKIP_COST=1 ./run-all.sh   # otra versión de Git
GIT_BIN_DIR=/ruta/a/git-2.56.0/bin REF_FORMAT=reftable SKIP_COST=1 ./run-all.sh
BENCH_N=100 bash suites/06-cost.sh        # coste: en una máquina en reposo, sin otras suites en paralelo
BENCH_N=100 bash suites/07-cost-native.sh # coste con dispatcher nativo; también en Git Bash (Windows)
```

- **Versiones de Git del spike**: Git 2.38.5 y 2.56.0 se compilaron desde las etiquetas `v2.38.5` y `v2.56.0` de `github.com/git/git` con `make prefix=<dir> NO_GETTEXT=1 NO_TCLTK=1 NO_CURL=1 NO_EXPAT=1 NO_PERL=1 NO_PYTHON=1 install`. La 2.50.1 es la de Apple (`/usr/bin/git`).
- `KEEP_SANDBOX=1` conserva los repos temporales para inspeccionarlos.

### Linux y Windows (no verificados en este spike)

- **Linux**: el mismo procedimiento. `sandbox_guard` y las huellas son POSIX. Comprobar sobre todo los casos con alias de mayúsculas (`F11`, `D08`), que en ext4 no aplican, y el coste del arranque de `sh`.
- **Windows**: ejecutar las suites desde Git Bash, con el `sh` de Git for Windows (que es el que ejecuta los hooks). La suite 06 debe compararse con un dispatcher que sea un ejecutable nativo, si Git for Windows lo admite (pregunta abierta del SPIKE). Los casos de alias (`F11`, `D08`) aplican en NTFS. El criterio de humano y agente (M-07) no se cubre aquí.
