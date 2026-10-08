# Registro diario de dogfooding (M1)

Instrumento de evidencia de los criterios 1, 2 y 5 de M1 ([backlog](../../docs/requirements/backlog.md#hito-m1--dogfooding), [plan de releases](../../docs/requirements/release-plan.md#m1-dogfooding)). Toma una muestra cada 15 minutos con la CLI pública y escribe un informe por día y la racha. Sin dependencias: Node 22 o posterior.

## Instalación (macOS, una línea)

Desde el checkout principal (no desde un worktree), con `raptor` y `node` en el `PATH`:

```sh
tools/dogfooding/install.sh
```

Instala un `LaunchAgent` de usuario (`~/Library/LaunchAgents/com.gitraptor.dogfooding.plist`, sin `sudo`) que ejecuta `sample.mjs` cada 15 minutos y al iniciar sesión. Es idempotente: si se vuelve a ejecutar, reemplaza el plist y recarga el agente. Opciones: `--interval <minutos>` (de 5 a 60) y `--dry-run` (muestra el plist y no instala nada). Para quitarlo: `tools/dogfooding/uninstall.sh` (conserva las muestras).

## Dónde queda cada cosa

| Qué | Dónde | Notas |
|---|---|---|
| Muestras | `~/.gitraptor-dogfooding/<fecha>.jsonl` | Fuera de todo repo y del perfil de GitRaptor (NFR-01): los scripts se niegan a escribir dentro de un repo o del perfil |
| Cursor, marcas y log | `~/.gitraptor-dogfooding/{state.json,marks.json,sampler.log}` | |
| Informe del día | `bitacora/dogfooding/<fecha>.md` | Local, no se commitea. Se regenera con cada muestra |
| Racha | `bitacora/dogfooding/racha.md` | Días laborables seguidos que cumplen el criterio 1 |

Las rutas se cambian con `--dir` y `--out` o con `GITRAPTOR_DOGFOODING_DIR` y `GITRAPTOR_DOGFOODING_REPORTS`.

## Qué consulta la muestra

Solo comandos de lectura de la CLI pública con `--json`, con argv fijo y sin shell:

1. `raptor status --resources --json`: daemon encendido, CPU media de la ventana, RSS y raíces observadas. **No arranca el motor.**
2. `raptor sessions --json` y `raptor events --json` (solo los eventos posteriores a la muestra anterior, con un cursor por repo): **solo si el daemon ya estaba encendido**, porque los dos arrancan el motor y falsearían el "daemon encendido" del criterio 1. Por la misma razón no se usa `raptor daemon status`, que además no tiene `--json`.

Ninguna muestra escribe en un repo ni ejecuta un comando reservado.

## El informe del día

- **Criterio 1 (uso real):** porcentaje de muestras con el daemon encendido en horario laboral (8:00 a 17:00) y máximo de sesiones de Claude Code presentes a la vez (activas o en espera). 🟢 si el daemon estuvo encendido en al menos el 90 % de las muestras y hubo al menos 3 sesiones en paralelo; 🟡 si cumple con menos de la mitad de las muestras esperadas (el Mac durmió); los fines de semana no cuentan.
- **Criterio 2 (detección):** eventos del día por actor (agente detectado, registrado, inferido y sin agente) y la proporción de detección de Claude Code: eventos atribuidos frente a eventos solo inferidos. 🟡 hasta que revisas el día; 🔴 si marcaste algún evento como tuyo o la detección baja del 90 %.
- **Criterio 5 (recursos):** CPU en reposo y RSS del daemon, mediana y p95, contra los objetivos que reporta la propia CLI (CPU < 1 %, RSS < 150 MiB).
- **Incidentes y notas:** por ejemplo, una recuperación real con `raptor undo` (criterio 3).

Hay un ejemplo generado a partir de muestras de prueba en [`examples/`](examples/racha.md).

## Lo que solo puedes decir tú (criterio 2)

"0 trabajo humano atribuido a Claude Code" no se puede saber con los datos del motor: si un evento atribuido a Claude Code lo hiciste tú, el motor no lo sabe. El informe lista los eventos atribuidos a Claude Code del día, agrupados por worktree, con un id `<repo>:<seq>`. Revísalos y marca los tuyos:

```sh
node tools/dogfooding/daily.mjs --mark e8343960:1302 human      # ese evento lo hice yo
node tools/dogfooding/daily.mjs --mark e8343960:1302 agent      # deshace la marca
node tools/dogfooding/daily.mjs --review --date 2026-10-09      # revisé el día
node tools/dogfooding/daily.mjs --note "undo real de un reset --hard" --date 2026-10-09
```

Las marcas y notas viven en `marks.json`, junto a las muestras, y sobreviven a la regeneración del informe.

## Limitaciones

- **Detección:** el motor no ve una sesión que no detectó y que tampoco dejó un evento inferido. La proporción del informe es una aproximación por eventos; la medición de sesiones del Research Brief de SPIKE-GRP-001 sigue siendo manual.
- **Sesiones cortas:** una sesión que empieza y termina entre dos muestras no se ve.
- **CPU "en reposo":** una muestra sin eventos Git desde la anterior. Un agente que edita archivos sin tocar Git también genera trabajo del observador.
- **Sueño del Mac:** `launchd` no ejecuta las muestras mientras el Mac duerme; el informe muestra cuántas muestras faltan.
- **Plataformas:** la programación con `launchd` es solo de macOS. Los scripts de Node funcionan en Linux y Windows con cron o el Programador de tareas, pero no se han verificado allí (*Pendiente: etapa de validación multiplataforma*).

## Desarrollo

```sh
node --test tools/dogfooding/dogfooding.test.mjs
UPDATE_EXAMPLES=1 node --test tools/dogfooding/dogfooding.test.mjs   # regenera examples/
node tools/dogfooding/daily.mjs --all                                # regenera todos los informes
```
