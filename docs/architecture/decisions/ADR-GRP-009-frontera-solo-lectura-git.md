---
id: ADR-GRP-009
title: Frontera de solo lectura e invocación del Git del sistema
type: adr
status: proposed
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-03
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-012, TS-GRP-002, INF-GRP-001, CTX-GRP-001, BR-CONS-001, BR-AUTH-002, BR-VAL-003, BR-WF-002]
tags: [git, gitoxide, solo-lectura, optional-locks, fsmonitor, untracked-cache, allowlist, argv, resolucion-git, nfr-01, nfr-07, br-cons-001]
---

# ADR-GRP-009 — Frontera de solo lectura e invocación del Git del sistema

## Contexto

El motor local **solo observa** (Q21): no escribe nada en el repo observado, ni código fuente ni rutas operativas, y fuera del repo solo escribe su perfil (Q17). La regla BR-CONS-001 acepta que Git produzca "efectos internos y temporales" al leer siempre que no cambien el estado observable, y delega en el Arquitecto la frontera técnica, con un criterio: **ante la duda, es modificación y el motor no la hace**. Q10 y Q13 fijan qué es código fuente y qué es ruta operativa; Q22 prohíbe en el MVP cualquier modificación operativa.

El problema es que muchos comandos de Git que parecen de lectura escriben por su cuenta:

- `git status` refresca la caché de stat del índice y reescribe `.git/index` tomando `index.lock`, y con la untracked cache activada la persiste en el índice.
- Con `core.fsmonitor` activado, `git status` arranca el daemon fsmonitor integrado, que crea un socket y archivos en `.git`, o ejecuta un hook fsmonitor configurado por el usuario.
- Varios comandos lanzan `gc --auto` o `maintenance --auto` y pueden escribir `commit-graph`, packs y `rerere`.
- Git ejecuta programas configurados por el usuario durante lecturas: filtros `clean` (p. ej. Git LFS, que escribe en `.git/lfs`), `textconv`, diff externo, `gpg` al mostrar firmas (escribe en `~/.gnupg`), pager, credential helpers y destinos de trace2.

El stack está fijado (ADR-GRP-001): gitoxide para leer y Git CLI del sistema para escribir. NFR-07 exige Git 2.38 o superior y Q28 dice que, si no se cumple, el motor avisa, no observa nada y empieza solo cuando Git aparece. El motor es un proceso en segundo plano por usuario, subcomando de `raptor` (ADR-GRP-005): arranca bajo launchd, `systemd --user` o el inicio de sesión de Windows con un PATH mínimo, distinto del de la shell. En macOS, `/usr/bin/git` es un shim que, sin las Command Line Tools, abre el diálogo de instalación del sistema: invocarlo sería una acción visible sobre la máquina del usuario.

## Decisión

Recomendación aceptada por Rene Bonilla el 2026-10-03 (índice de ADRs, opción 3). La capa vive en `crates/git` y es la única del monorepo que toca repos observados.

### 1. Lectura mixta

- **Camino caliente con gitoxide** (`gix`), abierto en solo lectura: refs, HEAD, objetos, índice, estado del working tree, ahead/behind y metadatos de worktrees. No se usa ninguna API de escritura de `gix` (índice, refs, config, objetos, locks). Los archivos se abren con modos que permiten a otros procesos borrarlos y renombrarlos (en Windows, `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`).
- **Git CLI solo como complemento**, para la versión (`git version`) y para las lecturas que gitoxide no cubra con fidelidad, siempre desde una **lista cerrada de subcomandos de lectura** con argv fijo (ver apartado 3).

### 2. Frontera: qué se permite y qué se prohíbe

**Principio**: el motor no provoca ninguna escritura en el repo observado ni fuera del perfil, ni siquiera transitoria (un `index.lock` creado y borrado cuenta), y **nunca ejecuta programas configurados por el usuario**. Esto es más estricto que la tolerancia de BR-CONS-001 para efectos internos y temporales: así la verificación puede exigir "cero diferencias" sin listas de excepciones.

| Efecto | Clasificación | Cómo se garantiza |
|---|---|---|
| Leer objetos, refs, `packed-refs`, índice, config, `HEAD`, reflogs y marcadores de operación en curso | Permitido | `gix` en solo lectura; `git` con `GIT_OPTIONAL_LOCKS=0` |
| Actualización de `atime` por el SO, cachés del SO, handles abiertos de lectura | Permitido (no es estado del repo) | Handles con borrado y renombrado compartidos; se cierran al terminar cada lectura |
| Refrescar la caché de stat del índice o reescribir `.git/index` | Prohibido | `GIT_OPTIONAL_LOCKS=0` y `--no-optional-locks`; `gix` sin escritura del índice |
| Escribir la untracked cache o la split index | Prohibido | Igual que arriba, más `-c core.untrackedCache=keep` y `-c core.splitIndex=false` |
| Arrancar el daemon fsmonitor de Git o ejecutar un hook fsmonitor | Prohibido | `-c core.fsmonitor=false`; `gix` sin fsmonitor |
| Crear cualquier `*.lock` (`index.lock`, `HEAD.lock`, `config.lock`, refs) | Prohibido | Sin subcomandos que los tomen; locks opcionales desactivados |
| `fetch`, `pull`, `push`, `gc`, `maintenance`, `repack`, `prune`, `commit-graph write`, `multi-pack-index write`, `rerere`, `worktree prune`/`repair`/`lock`/`unlock` | Prohibido | Fuera de la allowlist; además `-c gc.auto=0` y `-c maintenance.auto=false` como defensa en profundidad |
| `merge-tree --write-tree` (escribe objetos en `.git/objects`) | Prohibido en el motor | Fuera de la allowlist (ver Consecuencias) |
| Ejecutar filtros `clean`/`smudge`/`process`, `textconv`, diff externo, `gpg`, pager, editor | Prohibido | `--no-ext-diff`, `--no-textconv`, `-c log.showSignature=false`, `GIT_PAGER=cat`, sin filtros en `gix`; ver nota de filtros |
| Consultar o escribir credenciales; prompts de terminal | Prohibido | Ningún comando de red; `GIT_TERMINAL_PROMPT=0`, `-c credential.helper=` vacío, `GIT_ASKPASS` y `SSH_ASKPASS` sin definir |
| Escribir trazas de Git (`trace2.*Target`, `GIT_TRACE*`) | Prohibido | Variables `GIT_TRACE*` y `GIT_TRACE2*` eliminadas del entorno y `-c trace2.normalTarget=` / `eventTarget=` / `perfTarget=` vacíos |
| Ejecutar hooks del repo | Prohibido | Ningún subcomando de la allowlist ejecuta hooks; el hook fsmonitor está desactivado |
| Añadir el repo a `safe.directory` u otra escritura en la config global | Prohibido (Q17) | Un repo rechazado por `safe.directory` se reporta "no disponible" (BR-EDGE-001) |

**Nota de filtros**: si un archivo con atributo `filter` (p. ej. LFS) tiene el stat distinto al del índice con el mismo tamaño, Git ejecutaría el filtro para comparar contenido. El motor no lo ejecuta y lo reporta como modificado. Acepta un falso positivo raro en "hay cambios sin commitear" a cambio de no ejecutar código del usuario que escribe en `.git`.

**Qué no provoca el motor y no se le imputa**: lo que Git o el usuario hacen por su cuenta. Un daemon fsmonitor que el usuario ya tenía arrancado, el `gc --auto` que dispara un commit de un agente, los hooks que se ejecutan durante el commit de un agente o del desarrollador, un IDE que refresca el índice. El motor observa esas escrituras (ADR-GRP-010); no las causa. La verificación separa ambos casos con una ejecución de control (ver Validación).

### 3. Invocación del Git CLI

- **Sin shell**: `std::process::Command` con el **ejecutable por ruta absoluta** (nunca una búsqueda relativa al cwd, que en Windows permitiría a un repo hostil colar un `git.exe`), argv fijo por subcomando y `--` antes de cualquier ruta. Las refs y rutas que vienen del repo se validan y nunca se interpretan como opciones. En Windows, `CREATE_NO_WINDOW`.
- **Allowlist cerrada**, expuesta como funciones tipadas de `crates/git` y no como texto libre: `version`, `rev-parse`, `for-each-ref`, `worktree list --porcelain -z`, `status --porcelain=v2 -z`, `rev-list --count --left-right`, `merge-base`, `log` con formato fijo, `cat-file --batch`, `ls-files`, `diff --name-status --no-ext-diff --no-textconv` y `config --get`/`--get-regexp`/`--list` en su forma de solo lectura. Añadir un subcomando exige revisar este ADR.
- **Opciones fijas en cada invocación**: `--no-optional-locks`, `-c core.fsmonitor=false`, `-c core.untrackedCache=keep`, `-c core.splitIndex=false`, `-c gc.auto=0`, `-c maintenance.auto=false`, `-c log.showSignature=false`, `-c credential.helper=`, `-c color.ui=false`, `-c core.pager=cat` y los destinos de trace2 vacíos.
- **Entorno controlado**: se hereda el del motor, pero se fijan `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat` y `LC_ALL=C`, y se eliminan `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_CONFIG_*`, `GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_TRACE*` y `GIT_TRACE2*`.
- **Tiempo máximo por invocación** y terminación del proceso hijo si se excede; el resultado es "no disponible temporalmente", nunca un dato inventado.
- **Auditoría**: cada argv ejecutado se puede registrar en el perfil en modo diagnóstico, para que el arnés compruebe que todos pertenecen a la allowlist.

### 4. Resolución y verificación de Git (Q28)

- **Candidatos, en orden**: (1) ruta explícita opcional en la configuración de nivel perfil (ADR-GRP-006/007); (2) el PATH heredado por el proceso; (3) rutas conocidas por SO:
  - macOS: `/opt/homebrew/bin/git`, `/usr/local/bin/git`, la de las Command Line Tools (`/Library/Developer/CommandLineTools/usr/bin/git`) y la de Xcode.
  - Linux: `/usr/bin/git`, `/usr/local/bin/git` y el perfil de Nix del usuario.
  - Windows: la ruta de instalación de Git for Windows en el registro (solo lectura), `%ProgramFiles%\Git\cmd\git.exe`, `%LOCALAPPDATA%\Programs\Git\cmd\git.exe` y los shims de Scoop.
- **Shim de macOS**: `/usr/bin/git` (y cualquier candidato que resuelva a él) solo se invoca si existe un toolchain de desarrollador real, comprobado leyendo el sistema de archivos (que exista el `git` de las Command Line Tools o de Xcode) y no ejecutando el shim. Si no existe, el candidato se descarta sin invocarlo y nunca se abre el diálogo de instalación.
- **Selección**: el primer candidato que existe, es ejecutable y responde a `git version` con 2.38 o superior. Si ninguno cumple, el motor pasa a "Esperando Git" (BR-WF-002) con el motivo (ausente o versión encontrada) para que la CLI y el Cockpit lo presenten.
- **Recomprobación**:
  - En "Esperando Git", de forma periódica (⚠️ **ASSUMPTION**: cada 30 s) y al cambiar los directorios de los candidatos.
  - Mientras observa, cuando cambia la ruta, el tamaño o el mtime del ejecutable elegido. Si deja de cumplir, vuelve a "Esperando Git" y lo ocurrido mientras tanto se reconcilia como hueco "sin atribuir" (supuesto S19, BR-EDGE-005).
- **gitoxide también espera**: con Git insuficiente no se observa nada, aunque `gix` pudiera leer (Q28: sin observación parcial).

## Alternativas consideradas

- **Solo Git CLI con lecturas endurecidas**: un proceso por consulta, con más latencia y más CPU con 10 o más worktrees (NFR-04, NFR-05), y más superficie para que un comando ejecute código del usuario. Se descarta como camino caliente.
- **Solo gitoxide**: no ejecuta nada externo y es el más seguro, pero alguna lectura de alto nivel puede no tener paridad exacta con Git. Q28 exige Git 2.38 igualmente, así que no ahorra la dependencia. Se mantiene como camino caliente, no como único.
- **Mixta (elegida)**: gitoxide para casi todo y CLI acotado a una allowlist con opciones fijas.
- **Tolerar efectos internos temporales** (refresco del índice con lock, tal como permite BR-CONS-001): más fidelidad del estado y menos falsos positivos, pero obliga a justificar cada efecto, choca con agentes que escriben a la vez (un `index.lock` del motor puede hacer fallar el `git add` de un agente) y la verificación necesitaría excepciones. Se descarta por "ante la duda, es modificación".

## Consecuencias

- ✅ BR-CONS-001 y BR-AUTH-002 se pueden verificar con un criterio binario: cero diferencias en el repo y fuera del perfil.
- ✅ El motor nunca toma un lock: no compite con los agentes ni con el desarrollador por `index.lock`, y no puede hacerles fallar un comando.
- ✅ El motor no ejecuta código del usuario ni del repo, lo que reduce la superficie de ataque de un repo hostil.
- ✅ Funciona con el PATH mínimo de launchd y systemd, y no abre el diálogo de instalación en un Mac sin Command Line Tools.
- ⚠️ Sin refresco del índice, `git status` y `gix` repiten la comparación de contenido de los archivos con stat sucio en cada recomputo, y el coste crece hasta que el usuario o un agente refresque el índice. **Mitigación**: caché de stat propia en memoria del motor (nunca en el repo), que ADR-GRP-010 usa para el recomputo incremental; el coste se mide en SPIKE-GRP-002.
- ⚠️ No ejecutar filtros da falsos positivos de "modificado" en archivos con filtro (p. ej. LFS) cuyo mtime cambió sin cambiar el contenido. **Mitigación**: escenario LFS en INF-GRP-001 para medir la frecuencia; si es alta, se evalúa leer el puntero LFS sin ejecutar el filtro.
- ⚠️ Forzar `core.fsmonitor=false` y desactivar `textconv` y filtros sobrescribe, solo dentro de las invocaciones del motor, configuración del usuario que NFR-07 pide respetar. **Mitigación**: nada se escribe; la configuración del usuario rige todas sus operaciones y las de sus agentes. Se documenta como interpretación de NFR-07: el motor respeta la configuración que gobierna escrituras y credenciales y neutraliza solo la que ejecuta programas al leer.
- ⚠️ `merge-tree --write-tree`, la razón de Git 2.38 en NFR-07, escribe objetos en `.git/objects`. Queda prohibido en el motor. **Mitigación**: la predicción de conflictos (Cockpit) tendrá que escribir esos objetos en un almacén alternativo dentro del perfil (`GIT_OBJECT_DIRECTORY` con el repo como alternate) o pedir un ADR propio; no es alcance de motor-local.
- ⚠️ En Windows, gitoxide puede mapear packs en memoria y un mapeo abierto impide que el `git gc` del usuario los borre. **Mitigación**: handles de repo de vida corta, liberación al detectar un `gc` o `maintenance` en curso, y escenario de `gc` concurrente en INF-GRP-001.
- ⚠️ Un repo de otro propietario que Git rechaza por `safe.directory` no se puede observar sin tocar la config global (Q17). **Mitigación**: se reporta "no disponible" con el motivo y cómo resolverlo, sin escribir nada.

## Validación

La valida **INF-GRP-001** (arnés "repo intacto"), que bloquea el merge de cualquier historia del motor:

1. **Huella antes y después** del árbol `.git` común, de `.git/worktrees/*` y de cada working tree: lista de rutas, tipo, tamaño, hash de contenido y mtime de archivos **y directorios**. El mtime de directorio detecta un lock creado y borrado. Se excluye `atime`.
2. **Huella fuera del repo**: config global y de sistema de Git, `~/.gnupg`, otros repos de la máquina y config de nivel perfil. Solo pueden cambiar los datos del motor en el perfil.
3. **Ejecución de control**: cada escenario se ejecuta dos veces, con y sin motor, con la misma secuencia de acciones del usuario o del agente. Se imputa al motor solo la diferencia entre ambas huellas. Así se separa lo que hace Git por sí mismo o la config del usuario (daemon fsmonitor ya arrancado, hooks en un commit del agente, `gc --auto`) de lo que provoca el motor.
4. **Escenarios mínimos**, en Windows, macOS y Linux, con repos temporales y nunca este repo:
   - Worktrees enlazados.
   - Rebase y merge en curso, HEAD separado.
   - `core.fsmonitor=true` con el daemon parado y arrancado, y untracked cache y split index activados.
   - Hooks presentes, incluido `post-index-change`.
   - Git LFS con filtros.
   - Commits firmados con `log.showSignature=true` en la config.
   - `trace2.eventTarget` configurado.
   - `gc` del usuario concurrente con la observación.
   - Repo rechazado por `safe.directory`.
5. **Allowlist**: el registro de argv del modo diagnóstico solo contiene subcomandos y opciones de la lista. Además, una comprobación estática en CI exige que `Command::new` aparezca solo en el módulo de invocación de `crates/git`.
6. **Resolución de Git**: tests con PATH mínimo; en macOS, un runner sin Command Line Tools comprueba que no se ejecuta `/usr/bin/git` (proceso no lanzado) y que el motor queda en "Esperando Git"; cambio de versión en caliente por debajo y por encima de 2.38.

Si un escenario muestra un efecto imputable al motor, es defecto crítico (BR-CONS-001) y este ADR se revisa antes de seguir.

## Referencias

- Requerimiento: `docs/requirements/features/motor-local/context.md` (Q10, Q12, Q13, Q17, Q21, Q22, Q28; supuesto S19).
- Reglas: BR-CONS-001 (frontera delegada al Arquitecto), BR-AUTH-002, BR-VAL-003, BR-WF-002, BR-EDGE-001, BR-EDGE-005.
- BRD: NFR-01, NFR-02 (principio de argv fijo), NFR-07.
- ADRs: ADR-GRP-001 (gitoxide para leer, Git CLI para escribir), ADR-GRP-002 (`crates/git`), ADR-GRP-005 (proceso por usuario y entorno heredado), ADR-GRP-006 (perfil), ADR-GRP-010 (consume esta capa).
- Historias técnicas: TS-GRP-002 (capa de lectura), INF-GRP-001 (arnés).
- Documentación de Git: `git(1)` (`GIT_OPTIONAL_LOCKS`, `--no-optional-locks`), `git-config(1)` (`core.fsmonitor`, `core.untrackedCache`, `gc.auto`, `maintenance.auto`, `safe.directory`), `gitattributes(5)` (filtros y `textconv`), `api-trace2`.
