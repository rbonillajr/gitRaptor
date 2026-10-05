---
id: ADR-GRP-009
title: Frontera de solo lectura e invocación del Git del sistema
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-05
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-012, ADR-GRD-001, ADR-CKP-001, ADR-CKP-002, ADR-CKP-003, SPIKE-CKP-001, TS-GRP-002, INF-GRP-001, CTX-GRP-001, BR-GRP-001]
tags: [git, gitoxide, solo-lectura, optional-locks, fsmonitor, untracked-cache, allowlist, argv, resolucion-git, nfr-01, nfr-07, br-cons-001, seguridad, filtros, entorno, secretos]
---

# ADR-GRP-009 — Frontera de solo lectura e invocación del Git del sistema

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

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

- **Camino caliente con gitoxide** (`gix`), abierto en solo lectura: refs, HEAD, objetos, índice, estado del working tree, ahead/behind y metadatos de worktrees. No se usa ninguna API de escritura de `gix` (índice, refs, config, objetos, locks). (Enmienda 2026-10-04, Cockpit: excepción acotada a objetos en la memoria del proceso para el merge en seco; ver la sección final.) Los archivos se abren con modos que permiten a otros procesos borrarlos y renombrarlos (en Windows, `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`).
- **`status` y `diff` se calculan siempre con `gix` sin filtros** (revisión de seguridad, H2). El Git CLI ejecuta filtros `clean` al comparar archivos con stat sucio según `.gitattributes` y la config del repo, que un agente puede escribir: sería ejecución de código fuera del sandbox del agente y con persistencia. Por eso `status` y `diff` salen de la allowlist del CLI.
- **`gix` configurado para no invocar el binario `git`** (M1): `gix` (vía `gix-path`) puede lanzar `git` por su cuenta para descubrir la config de instalación; se desactiva con sus opciones de apertura y permisos. ⚠️ **Pendiente de comprobación** en la versión fijada del crate: qué opciones lo garantizan y que ninguna ruta de código lo eluda. La auditoría dinámica de `exec` de INF-GRP-001 lo verifica.
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
| `merge-tree --write-tree` (escribe objetos en `.git/objects`) | Prohibido en el motor | Fuera de la allowlist (ver Consecuencias). Enmienda 2026-10-04, Cockpit: dos filas nuevas para el merge en seco en la sección final |
| Ejecutar filtros `clean`/`smudge`/`process`, `textconv`, diff externo, `gpg`, pager, editor | Prohibido | `status`/`diff` solo con `gix` sin filtros; en el CLI `--no-ext-diff`, `--no-textconv`, `-c log.showSignature=false`, `GIT_PAGER=cat` y `log` sin placeholders `%G*`; ver nota de filtros |
| Consultar o escribir credenciales; prompts de terminal | Prohibido | Ningún comando de red; `GIT_TERMINAL_PROMPT=0`, `-c credential.helper=` vacío, `GIT_ASKPASS` y `SSH_ASKPASS` sin definir |
| Escribir trazas de Git (`trace2.*Target`, `GIT_TRACE*`) | Prohibido | Variables `GIT_TRACE*` y `GIT_TRACE2*` fuera del entorno de los hijos (allowlist) y `-c trace2.normalTarget=` / `eventTarget=` / `perfTarget=` vacíos |
| Ejecutar hooks del repo | Prohibido | Ningún subcomando de la allowlist ejecuta hooks; el hook fsmonitor está desactivado |
| Añadir el repo a `safe.directory` u otra escritura en la config global | Prohibido (Q17) | Un repo rechazado por `safe.directory` se reporta "no disponible" (BR-EDGE-001); `gix` aplica el mismo criterio de propiedad y nunca se pasa `-c safe.directory=*` (SEC-11) |
| Leer o exponer secretos (valores de config, tokens en URLs, `http.*.extraHeader`, entorno) | Prohibido (SEC-05) | Solo `config --get` de claves tipadas de una allowlist; URLs de remotos guardadas y expuestas sin userinfo; nada de esto va a logs, perfil ni canal |
| Tocar rutas UNC o de red (Windows) | Prohibido sin acción explícita del usuario | Rutas UNC, `\\?\` y de dispositivo se rechazan antes de tocar el FS; abrirlas filtraría el hash NTLM (M9) |

**Nota de filtros**: si un archivo con atributo `filter` (p. ej. LFS) tiene el stat distinto al del índice con el mismo tamaño, Git ejecutaría el filtro para comparar contenido. El motor calcula `status` con `gix` sin filtros: no lo ejecuta y lo reporta como modificado. Acepta un falso positivo raro en "hay cambios sin commitear" a cambio de no ejecutar código del usuario que escribe en `.git`.

**Qué no provoca el motor y no se le imputa**: lo que Git o el usuario hacen por su cuenta. Un daemon fsmonitor que el usuario ya tenía arrancado, el `gc --auto` que dispara un commit de un agente, los hooks que se ejecutan durante el commit de un agente o del desarrollador, un IDE que refresca el índice. El motor observa esas escrituras (ADR-GRP-010); no las causa. La verificación separa ambos casos con una ejecución de control (ver Validación).

### 3. Invocación del Git CLI

- **Sin shell**: `std::process::Command` con el **ejecutable por ruta absoluta** (nunca una búsqueda relativa al cwd, que en Windows permitiría a un repo hostil colar un `git.exe`), argv fijo por subcomando y `--` antes de cualquier ruta. Las refs y rutas que vienen del repo se validan y nunca se interpretan como opciones. En Windows, `CREATE_NO_WINDOW`.
- **Allowlist cerrada**, expuesta como funciones tipadas de `crates/git` y no como texto libre: `version`, `rev-parse`, `for-each-ref`, `worktree list --porcelain -z`, `rev-list --count --left-right`, `merge-base`, `log` con formato fijo (sin placeholders `%G*`, que invocarían `gpg`), `cat-file --batch`, `ls-files` y `config --get` **solo de claves tipadas de una allowlist** (sin `--list` ni `--get-regexp`, que expondrían tokens en `remote.*.url` o `http.*.extraHeader`, H3). `status` y `diff` no están en la allowlist: los calcula `gix` sin filtros (H2). Añadir un subcomando o una clave exige revisar este ADR.
- **Secretos (SEC-05)**: las URLs de remotos se guardan y se exponen sin userinfo; ningún valor de config, entorno ni salida de `git` va a logs, perfil o canal salvo los campos tipados que el motor necesita.
- **Opciones fijas en cada invocación**: `--no-optional-locks`, `-c core.fsmonitor=false`, `-c core.untrackedCache=keep`, `-c core.splitIndex=false`, `-c gc.auto=0`, `-c maintenance.auto=false`, `-c log.showSignature=false`, `-c credential.helper=`, `-c color.ui=false`, `-c core.pager=cat` y los destinos de trace2 vacíos.
- **Entorno controlado por allowlist (SEC-10, H4)**: el hijo `git` **no hereda** el entorno del motor; recibe uno construido desde cero con solo `HOME`, `PATH` (sin entradas relativas), las variables de sistema imprescindibles por SO (`SystemRoot`, `USERPROFILE` y similares en Windows) y las fijas `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat` y `LC_ALL=C`. Cualquier otra variable (`GIT_*`, `GIT_EXEC_PATH`, `GIT_SSH_COMMAND`, `GIT_ATTR_SOURCE`, `GIT_COMMON_DIR`, `GIT_CEILING_DIRECTORIES`, `GIT_REPLACE_REF_BASE`, `LD_PRELOAD`, `DYLD_*`, `XDG_CONFIG_HOME`…) queda fuera por construcción. Una denylist se descarta porque siempre queda incompleta.
- **Tiempo máximo por invocación** y terminación del proceso hijo si se excede; el resultado es "no disponible temporalmente", nunca un dato inventado.
- **Auditoría**: cada argv ejecutado se puede registrar en el perfil en modo diagnóstico, para que el arnés compruebe que todos pertenecen a la allowlist.

### 4. Resolución y verificación de Git (Q28)

- **Candidatos, en orden**: (1) ruta explícita opcional en la configuración de nivel perfil (`engine.gitPath`, ADR-GRP-006/007); (2) el PATH heredado por el proceso, ignorando entradas relativas; (3) rutas conocidas por SO:
  - macOS: `/opt/homebrew/bin/git`, `/usr/local/bin/git`, la de las Command Line Tools (`/Library/Developer/CommandLineTools/usr/bin/git`) y la de Xcode.
  - Linux: `/usr/bin/git`, `/usr/local/bin/git` y el perfil de Nix del usuario.
  - Windows: la ruta de instalación de Git for Windows en el registro (solo lectura), `%ProgramFiles%\Git\cmd\git.exe`, `%LOCALAPPDATA%\Programs\Git\cmd\git.exe` y los shims de Scoop.
- **Shim de macOS**: `/usr/bin/git` (y cualquier candidato que resuelva a él) solo se invoca si existe un toolchain de desarrollador real, comprobado leyendo el sistema de archivos (que exista el `git` de las Command Line Tools o de Xcode) y no ejecutando el shim. Si no existe, el candidato se descarta sin invocarlo y nunca se abre el diálogo de instalación.
- **Validación del ejecutable (SEC-10, M10)**: antes de invocarlo, todo candidato (también la ruta explícita) debe ser **absoluto, archivo regular, propiedad del usuario o de root y no escribible por grupo ni otros** (en Windows, regla de propietario y DACL de la Enmienda (2026-10-05, TD-GRP-001)). Un candidato que no cumple se descarta sin ejecutarlo y el motivo se reporta.
- **Selección**: el primer candidato válido que es ejecutable y responde a `git version` con 2.38 o superior. Si ninguno cumple, el motor pasa a "Esperando Git" (BR-WF-002) con el motivo (ausente o versión encontrada) para que la CLI y el Cockpit lo presenten. (Enmienda 2026-10-04, Cockpit: el ejecutor de operaciones de usuario usa este mismo binario.)
- **Recomprobación**:
  - En "Esperando Git", de forma periódica (⚠️ **ASSUMPTION**: cada 30 s) y al cambiar los directorios de los candidatos.
  - Mientras observa, cuando cambia la ruta, el tamaño o el mtime del ejecutable elegido. Si deja de cumplir, vuelve a "Esperando Git" y lo ocurrido mientras tanto se reconcilia como hueco "sin atribuir" (supuesto S19, BR-EDGE-005).
- **gitoxide también espera**: con Git insuficiente no se observa nada, aunque `gix` pudiera leer (Q28: sin observación parcial).

## Alternativas consideradas

- **Solo Git CLI con lecturas endurecidas**: un proceso por consulta, con más latencia y más CPU con 10 o más worktrees (NFR-04, NFR-05), y más superficie para que un comando ejecute código del usuario. Se descarta como camino caliente.
- **Solo gitoxide**: no ejecuta nada externo y es el más seguro, pero alguna lectura de alto nivel puede no tener paridad exacta con Git. Q28 exige Git 2.38 igualmente, así que no ahorra la dependencia. Se mantiene como camino caliente, no como único.
- **Mixta (elegida)**: gitoxide para casi todo, incluidos siempre `status` y `diff` sin filtros, y CLI acotado a una allowlist con opciones fijas.
- **`status`/`diff` con el CLI y `GIT_ATTR_SOURCE` apuntando al árbol vacío**: neutraliza los atributos del working tree y por tanto los filtros, pero exige Git 2.40 o superior. Q28 y NFR-07 fijan el mínimo en 2.38, así que se descarta; no se sube el mínimo de Git (revisión de seguridad, H2).
- **Entorno de los hijos por denylist**: la versión anterior de este ADR eliminaba una lista de variables; la revisión de seguridad (H4) encontró huecos (`GIT_EXEC_PATH`, `GIT_SSH_COMMAND`, `LD_PRELOAD`, `DYLD_*`…). Se sustituye por allowlist.
- **Tolerar efectos internos temporales** (refresco del índice con lock, tal como permite BR-CONS-001): más fidelidad del estado y menos falsos positivos, pero obliga a justificar cada efecto, choca con agentes que escriben a la vez (un `index.lock` del motor puede hacer fallar el `git add` de un agente) y la verificación necesitaría excepciones. Se descarta por "ante la duda, es modificación".

## Consecuencias

- ✅ BR-CONS-001 y BR-AUTH-002 se pueden verificar con un criterio binario: cero diferencias en el repo y fuera del perfil.
- ✅ El motor nunca toma un lock: no compite con los agentes ni con el desarrollador por `index.lock`, y no puede hacerles fallar un comando.
- ✅ El motor no ejecuta código del usuario ni del repo, lo que reduce la superficie de ataque de un repo hostil; en particular, un agente que escribe `.gitattributes` o la config del repo no consigue que el motor ejecute un filtro (H2).
- ✅ Funciona con el PATH mínimo de launchd y systemd, y no abre el diálogo de instalación en un Mac sin Command Line Tools.
- ⚠️ Sin refresco del índice, `gix` repite la comparación de contenido de los archivos con stat sucio en cada recomputo, y el coste crece hasta que el usuario o un agente refresque el índice. **Mitigación**: caché de stat propia en memoria del motor (nunca en el repo), que ADR-GRP-010 usa para el recomputo incremental; el coste se mide en SPIKE-GRP-002.
- ⚠️ No ejecutar filtros da falsos positivos de "modificado" en archivos con filtro (p. ej. LFS) cuyo mtime cambió sin cambiar el contenido. **Mitigación**: escenario LFS en INF-GRP-001 para medir la frecuencia; si es alta, se evalúa leer el puntero LFS sin ejecutar el filtro.
- ⚠️ Forzar `core.fsmonitor=false` y desactivar `textconv` y filtros sobrescribe, solo dentro de las invocaciones del motor, configuración del usuario que NFR-07 pide respetar. **Mitigación**: nada se escribe; la configuración del usuario rige todas sus operaciones y las de sus agentes. Se documenta como interpretación de NFR-07: el motor respeta la configuración que gobierna escrituras y credenciales y neutraliza solo la que ejecuta programas al leer.
- ⚠️ `merge-tree --write-tree`, la razón de Git 2.38 en NFR-07, escribe objetos en `.git/objects`. Queda prohibido en el motor. **Mitigación**: la predicción de conflictos (Cockpit) tendrá que escribir esos objetos en un almacén alternativo dentro del perfil (`GIT_OBJECT_DIRECTORY` con el repo como alternate) o pedir un ADR propio; no es alcance de motor-local. **Resuelta (Enmienda 2026-10-04, Cockpit)** por ADR-CKP-001: merge en memoria con `gix`, sin escribir objetos, pendiente de SPIKE-CKP-001.
- ⚠️ En Windows, gitoxide puede mapear packs en memoria y un mapeo abierto impide que el `git gc` del usuario los borre. **Mitigación**: handles de repo de vida corta, liberación al detectar un `gc` o `maintenance` en curso, y escenario de `gc` concurrente en INF-GRP-001.
- ⚠️ Un repo de otro propietario que Git rechaza por `safe.directory` no se puede observar sin tocar la config global (Q17). **Mitigación**: se reporta "no disponible" con el motivo y cómo resolverlo, sin escribir nada.

Nota de integración (Time Machine, ADR-TMC-002, aceptado el 2026-10-03): la frontera de solo lectura de este ADR es la del motor. `crates/git` aloja además una segunda lista cerrada, de escritura, que solo usa el módulo `timemachine` de `crates/core` (SEC-TMC-02, SEC-TMC-14). La comprobación estática de la Validación 5 se amplía: ni el observador del motor ni el ejecutor de operaciones de usuario pueden importar la capa de escritura de la Time Machine.

Nota de integración (Guardrails, ADR-GRD-001 § 7; Enmienda 2026-10-04): `crates/git` aloja una **segunda capa de escritura, separada**, la de Guardrails, análoga a la de la Time Machine. Es una lista cerrada de operaciones tipadas: escribir, restaurar o eliminar `core.hooksPath` en el `config` del directorio común; leer el valor efectivo de `core.hooksPath` por worktree; y crear, sustituir y borrar archivos listados solo dentro de `<git-common-dir>/gitraptor/`. Esas operaciones solo las alcanza el módulo `guardrails` de `crates/core` (visibilidad de módulo y frontera de Nx), con el Git CLI, argv fijo, sin shell, con el entorno por allowlist del § 3 y sin ejecutar hooks ni filtros. La capa tiene además **dos módulos de invocación autorizados, nombrados y tipados** (decisión del Arquitecto, 2026-10-04; ADR-GRD-001 § 7): uno **encadena el hook previo**, sin shell, y otro **ejecuta `git` para `raptor guard exec`**, con el argv validado y normalizado según ADR-GRD-007 § 3. `raptor hook` y `raptor guard exec` (`apps/cli`) los llaman y no tienen un `Command::new` propio. **No es una escritura del motor**: solo ocurre tras una instalación, desinstalación, actualización o adopción explícitas, que son comandos reservados (ADR-GRP-005 § 6). La frontera de solo lectura de este ADR sigue siendo la del motor, y INF-GRP-001 admite esas escrituras solo en sus escenarios de instalación. La comprobación estática de la Validación 5 se amplía también a esta capa.

Nota de integración (INF-GRP-001, 2026-10-04; decisión del orquestador, validada por el Arquitecto): la Validación se implementa en `crates/testkit` y en los tests `repo_intact::` de `crates/git` (Dev Spec `DS-INF-GRP-001`).
- **Validación 2**: la config de sistema se resuelve preguntando al Git instalado, no con una ruta fija.
- **Validación 5**: la comprobación estática sigue cubriendo solo `crates/git/src`. El testkit queda fuera porque solo puede ser dev-dependency y no es código del motor.
- **Validación 7**: el gate portable de los tres SO es una auditoría por trampas sin privilegios (shim de `git` que registra el argv desde el hijo y trampas en el `PATH`). eslogger, strace y ETW quedan como auditoría profunda. A 2026-10-04 solo está verificada la auditoría por trampas en macOS (eslogger, strace y ETW sin verificar; ETW sin implementar), así que la condición de esta Validación sigue **abierta**.

Nota de integración (Windows, 2026-10-05; decisión del orquestador, validada por el Arquitecto): en Windows, `std::fs::canonicalize` devuelve rutas verbatim (`\\?\C:\…`), y el § 2 las rechaza por ser verbatim. Quien canonicaliza una ruta que luego llega a la capa de lectura (`observe::locate`, la clave de repo del perfil) usa `gitraptor_git::paths::canonicalize`, que devuelve la forma con letra de unidad cuando nombra el mismo archivo y supera el § 2. Una ruta sin esa forma (`\\?\UNC\…`, GUID de volumen) o que nombraría otro archivo (componente con punto o espacio final) se queda verbatim y se rechaza. El rechazo léxico de UNC, verbatim y dispositivos no cambia.

Nota de integración (Cockpit, ADR-CKP-002; Enmienda 2026-10-04): `crates/git` aloja una **tercera capa de escritura**, la invocación de operaciones de usuario del ejecutor del daemon. Detalle en la sección final.

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
5. **Allowlist**: el registro de argv del modo diagnóstico solo contiene subcomandos y opciones de la lista. Además, una comprobación estática en CI exige que `Command::new` aparezca solo en el módulo de invocación de `crates/git`. **Ampliada** (notas de integración de Time Machine y de Guardrails): el lanzamiento de procesos solo está en los módulos de invocación autorizados de `crates/git`, que la comprobación lista por nombre: lectura del motor, escritura de la Time Machine, escritura de Guardrails y los dos módulos de invocación de Guardrails (encadenado del hook previo y `git` de `raptor guard exec`). `raptor hook` y `raptor guard exec` no tienen un `Command::new` propio. Ni el observador del motor ni el ejecutor de operaciones de usuario pueden importar ninguna de las dos capas de escritura; las operaciones de escritura de Guardrails solo las importa el módulo `guardrails`, y sus dos módulos de invocación solo `raptor hook` y `raptor guard exec`. **Ampliada otra vez** (Enmienda 2026-10-04, Cockpit): invocación de operaciones de usuario, lanzador del editor y arranque del daemon; ver la sección final.
6. **Resolución de Git**: tests con PATH mínimo; en macOS, un runner sin Command Line Tools comprueba que no se ejecuta `/usr/bin/git` (proceso no lanzado) y que el motor queda en "Esperando Git"; cambio de versión en caliente por debajo y por encima de 2.38.
7. **Cero ejecución de código configurable (SEC-09)**: repo canario con `filter.*.clean`, `diff.*.textconv`, `core.fsmonitor`, hooks y `gpg.program` apuntando a un script que deja un marcador; con archivos de stat sucio y el motor observando, el marcador nunca aparece. **Auditoría dinámica de `exec`** en INF-GRP-001 (eslogger en macOS, ETW en Windows, strace en Linux): todo proceso hijo del motor pertenece a la allowlist y `gix` no lanza `git` (M1). (Enmienda 2026-10-04, Cockpit: repo canario ampliado para el merge en seco.)
8. **Secretos (SEC-05)**: suite con secretos plantados (`.env`, token en la URL del remoto, `http.extraHeader`) y gitleaks/trufflehog sobre perfil, logs y captura del stream IPC: 0 hallazgos. (Enmienda 2026-10-04, Cockpit: alcance del escaneo frente a la consulta de diff; ver la sección final.)
9. **Entorno y ejecutable (SEC-10)**: motor arrancado con `GIT_EXEC_PATH`, `GIT_SSH_COMMAND`, `LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`, `PATH=.:…` o `XDG_CONFIG_HOME` hostiles: sin efecto en los hijos; un `git` escribible por todos o una `engine.gitPath` relativa se rechazan.
10. **Repos no confiables (SEC-11, SEC-02)**: repo de otro uid → "no disponible" sin `safe.directory=*`; ruta UNC → 0 conexiones SMB; ref `--upload-pack=x` rechazada.

Si un escenario muestra un efecto imputable al motor, es defecto crítico (BR-CONS-001) y este ADR se revisa antes de seguir.

## Referencias

- Requerimiento: `docs/requirements/features/motor-local/context.md` (Q10, Q12, Q13, Q17, Q21, Q22, Q28; supuesto S19).
- Reglas: BR-CONS-001 (frontera delegada al Arquitecto), BR-AUTH-002, BR-VAL-003, BR-WF-002, BR-EDGE-001, BR-EDGE-005.
- BRD: NFR-01, NFR-02 (principio de argv fijo), NFR-07.
- ADRs: ADR-GRP-001 (gitoxide para leer, Git CLI para escribir), ADR-GRP-002 (`crates/git`), ADR-GRP-005 (proceso por usuario y entorno heredado), ADR-GRP-006 (perfil), ADR-GRP-010 (consume esta capa).
- Historias técnicas: TS-GRP-002 (capa de lectura), INF-GRP-001 (arnés).
- Documentación de Git: `git(1)` (`GIT_OPTIONAL_LOCKS`, `--no-optional-locks`, `GIT_ATTR_SOURCE`), `git-config(1)` (`core.fsmonitor`, `core.untrackedCache`, `gc.auto`, `maintenance.auto`, `safe.directory`), `gitattributes(5)` (filtros y `textconv`), `api-trace2`.
- Seguridad: `docs/architecture/non-functional.md` (SEC-02, SEC-05, SEC-09, SEC-10, SEC-11).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. Mantiene la lectura mixta y el mínimo de Git 2.38 (Q28, NFR-07).

| Hallazgo | Cómo se cubre |
|---|---|
| H2 · `status`/`diff` del CLI ejecutan filtros `clean` | Apartado 1: `status` y `diff` siempre con `gix` sin filtros; salen de la allowlist del CLI. `GIT_ATTR_SOURCE` (Git ≥ 2.40) queda como alternativa descartada por Q28/NFR-07 (SEC-09) |
| H3 · `config --list`/`--get-regexp` exponen tokens | Apartado 3: solo `config --get` de claves tipadas en allowlist; userinfo de URLs redactado (SEC-05) |
| H4 · Denylist de entorno incompleta | Apartado 3: entorno de los hijos construido por allowlist, PATH sin entradas relativas (SEC-10); arranque del daemon con entorno limpio en ADR-GRP-005 |
| M1 · `gix` puede lanzar `git` por su cuenta | Apartado 1: `gix` configurado para no invocar `git`, ⚠️ pendiente de comprobar en la versión fijada del crate; auditoría dinámica de `exec` en INF-GRP-001 |
| M9 · UNC en Windows | Apartado 2: rutas UNC y de dispositivo rechazadas antes de tocar el FS (SEC-02, SEC-11) |
| M10 · Ruta explícita de Git sin validar | Apartado 4: ejecutable absoluto, regular, de usuario o root y no escribible por grupo/otros (SEC-10); `engine.gitPath` en ADR-GRP-007 |
| L2 · `log` con `%G*` invoca `gpg` | Apartado 3: `log` con formato fijo sin `%G*` (SEC-09) |

Validación ampliada: SEC-02, SEC-05, SEC-09, SEC-10 y SEC-11 (puntos 7 a 10). Condición para pasar a `accepted`: esos puntos en la Validación (cubierto en texto) e INF-GRP-001 con repo canario y auditoría dinámica de `exec`.

## Enmienda (2026-10-04, Guardrails)

Aplicada desde la tabla de enmiendas de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10). No cambia la frontera de solo lectura del motor. El `status` siguió en `proposed` hasta su aceptación (Rene Bonilla, 2026-10-04).

| Cambio | Dónde | Fuente |
|---|---|---|
| Nota de una segunda capa de escritura separada, la de Guardrails, como la de la Time Machine (TQ-12) | Nota de integración tras Consecuencias | ADR-GRD-001 § 7 |
| Comprobación estática de la Validación 5 ampliada a esa capa: lanzamiento de procesos solo desde los módulos de invocación autorizados | Validación 5 | ADR-GRD-001 § 7 |
| **Ronda de coherencia (2026-10-04)**: dos módulos de invocación autorizados de Guardrails (encadenado del hook previo y `git` de `raptor guard exec`), llamados desde `apps/cli` sin `Command::new` propio y listados en la comprobación estática | Nota de integración; Validación 5 | Decisión del Arquitecto (2026-10-04); ADR-GRD-001 § 7 |

**Pendiente de motor-local** (anotado el 2026-10-04, no se decide en esta enmienda): la regla de la Validación 5 ("`Command::new` solo en los módulos de invocación autorizados de `crates/git`") choca con lanzamientos de procesos que ya existían en ADR-GRP-005 § 3: el arranque del daemon bajo demanda desde la biblioteca cliente y las peticiones al gestor de servicios (`launchctl kickstart`, `systemctl --user start`). Es una tensión previa de motor-local; falta decidir en qué módulo autorizado viven esos lanzamientos. **Cerrado por la Enmienda (2026-10-04, Cockpit)**: viven en el módulo de arranque de la biblioteca cliente de `crates/api`.

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-1, 3, 7 y 12 de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-001](./ADR-CKP-001-prediccion-conflictos-merge-en-seco.md) (opción preferida (a)), [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) y la enmienda E3 de [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md), los tres `accepted` el 2026-10-04. **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. **No cambia la frontera de solo lectura del motor ni su criterio binario**: cero diferencias en el repo y cero programas configurados por el usuario. El `status` sigue en `accepted`. Lo que depende de la opción (b) de ADR-CKP-001 **no se aplica**: queda condicionado a SPIKE-CKP-001 (abajo).

| Cambio | Dónde | Fuente |
|---|---|---|
| Excepción acotada a "ninguna API de escritura de `gix`": el módulo de merge en seco escribe objetos **solo en la memoria del proceso** | § 1 | DEP-CKP-1; ADR-CKP-001 § 1 (a) |
| Dos filas nuevas en la tabla: merge en memoria con `gix` (permitido) y drivers de merge o filtros durante el merge en seco (prohibido). `merge-tree --write-tree` sigue prohibido | § 2 | ADR-CKP-001 § 1 (a) |
| § 3 sin cambios con la opción (a) | § 3 | ADR-CKP-001 |
| El ejecutor de operaciones de usuario usa el mismo binario resuelto (une la nota E6 de ADR-TMC-002) | § 4 | DEP-CKP-7; ADR-CKP-002 § 6 |
| Tercera capa de escritura separada: invocación de operaciones de usuario | Nota de integración | DEP-CKP-7; ADR-CKP-002 § 6 y § 11 |
| Lista de módulos que pueden lanzar procesos, dentro y fuera de `crates/git` (editor y arranque del daemon) | Validación 5 | DEP-CKP-7, DEP-CKP-12; ADR-CKP-002 § 10; ADR-CKP-003 § 5, § 9 (E3) |
| Repo canario ampliado con drivers de merge, filtros de proceso y `.gitattributes` con `merge=` | Validación 7 | DEP-CKP-1; ADR-CKP-001, Validación 2 y 3 |
| Alcance del escaneo de secretos frente a la consulta de diff | Validación 8 | DEP-CKP-3; Q-CKP-8 |
| La consecuencia sobre `merge-tree` queda resuelta por ADR-CKP-001 | Consecuencias | DEP-CKP-1 |
| Cierra el pendiente de motor-local sobre lanzar procesos fuera de `crates/git` (y el punto 5 del § 10 del overview) | Enmienda (Guardrails) | DEP-CKP-12; ADR-CKP-003 (E3) |

### Merge en seco con `gix` en memoria (opción (a) de ADR-CKP-001)

- **Excepción acotada**: solo el módulo de merge en seco de `crates/git` abre una instancia con objetos en memoria (`Repository::with_object_memory`). Los árboles y blobs que produce el merge se quedan en la memoria del proceso y se descartan al terminar. Nunca llegan a `.git/objects`, ni sueltos ni en packs. El resto de la capa sigue sin ninguna API de escritura de `gix`.
- **Condiciones obligatorias**: lista de drivers de merge vacía y sin contexto de invocación; sin procesos de filtro (`clean`, `smudge`, `process`); pila de atributos sin `.gitattributes` del disco; sin invocar el binario `git` (M1); sin descargas perezosas de un *partial clone* (un objeto ausente da "no calculable").
- **Entradas tipadas**: recibe ids de commit ya resueltos, nunca nombres de ref de texto libre.

Filas nuevas de la tabla del § 2:

| Efecto | Clasificación | Cómo se garantiza |
|---|---|---|
| Merge de árboles en memoria con `gix` para la predicción de conflictos | Permitido | Instancia con objetos en memoria, solo en el módulo de merge en seco; nada se escribe en disco |
| Ejecutar drivers de merge (`merge.<driver>.driver`) o procesos de filtro durante el merge en seco | Prohibido | Drivers vacíos y sin contexto de invocación; sin filtros ni atributos del disco (SEC-09) |

**Condicionado a SPIKE-CKP-001 (opción (b), no aplicado)**. Si el SPIKE activa el respaldo (b), `merge-tree --write-tree` sobre un almacén de trabajo en el perfil, se abrirá una enmienda nueva con estos cambios: la fila de `merge-tree` pasa a "permitido solo con `--git-dir` en el almacén de trabajo del perfil", más una fila para el refresco de objetos (mtime) a través de los alternates; § 3 añade `merge-tree` a la allowlist con argv fijo y el entorno aislado de la configuración global y de sistema; la Validación 5 añade su módulo de invocación; la Validación 7 añade un *partial clone* con remoto *promisor* y captura de red. Mientras tanto, nada de esto rige.

### Ejecutor de operaciones de usuario (DEP-CKP-7)

- **Tercera capa de escritura, separada**, como las de la Time Machine y Guardrails: el **módulo de invocación de operaciones de usuario** de `crates/git`, con una lista cerrada y tipada (merge de un oid, abortar merge, rebase sobre un oid, abortar rebase, añadir worktree con rama nueva, quitar worktree, actualizar ref con valor viejo y borrar ref con valor viejo). Solo lo importa el módulo `executor` de `crates/core` (ADR-CKP-002 § 11).
- **No es una escritura del motor**: solo ocurre cuando un actor la pide, dentro de la operación protegida (ADR-TMC-004 § 1), después de la decisión de Guardrails. La frontera de solo lectura de este ADR sigue siendo la del observador, del predictor y de las consultas. INF-GRP-001 admite esas escrituras solo en los escenarios del ejecutor.
- **Respeta lo del usuario** (NFR-07, ADR-TMC-002 § 5): hooks, filtros, drivers de merge, `rerere`, identidad y firma. Neutraliza solo los ejecutables de ADR-GRD-007 § 3.5 y los editores, y desactiva `gc.auto` y `maintenance.auto` (ADR-CKP-002 § 6).
- **Entorno**: la allowlist del § 3, más las variables fijas del ejecutor y la lista cerrada de variables de sesión que el cliente declara y el daemon valida (ADR-CKP-002 § 6).
- **Binario**: el mismo que resuelve el § 4, una vez y con ruta absoluta.

### Validación 5 ampliada: quién puede lanzar procesos

1. **En `crates/git`**, módulos de invocación autorizados y nombrados: lectura del motor; escritura de la Time Machine; escritura de Guardrails y sus dos módulos de invocación; **invocación de operaciones de usuario**, que solo importa `crates/core::executor`.
2. **Fuera de `crates/git`** (decisión que cierra el pendiente de motor-local y el punto 5 del § 10 del overview):
   - el **lanzador del editor** `tui::editor` de `apps/cli`, que solo ejecuta un argv ya validado por la función pura de `crates/api`, con `argv[0]` resuelto a ruta absoluta con un PATH sin entradas relativas ni el cwd, y sin shell. Es el único `Command::new` de la TUI;
   - el **módulo de arranque de la biblioteca cliente de `crates/api`**, que arranca el daemon bajo demanda con entorno limpio y hace las peticiones al gestor de servicios (`launchctl kickstart`, `systemctl --user start`) de ADR-GRP-005 § 3.
3. **Importaciones prohibidas**: `executor` no importa ninguna capa de escritura (Time Machine, Guardrails) ni los módulos de invocación de Guardrails; nadie más que `executor` importa la invocación de operaciones de usuario; el predictor de ADR-CKP-001 no importa ninguna capa de escritura; el módulo de merge en seco usa siempre la instancia en memoria.

El "ejecutable de rechazo" que el ejecutor pone en `GIT_EDITOR` (ADR-CKP-002 § 6) lo lanza Git, no GitRaptor, y no añade un lanzamiento.

### Validación 7 ampliada (SEC-09)

Repo canario con `merge.<x>.driver`, `filter.<x>.process` y `filter.<x>.clean`, `.gitattributes` con `merge=<x>` y `core.fsmonitor`, todos apuntando a un script que deja un marcador. Con el predictor calculando todos los pares, el marcador nunca aparece y la auditoría dinámica de `exec` muestra 0 procesos lanzados por el predictor. Un *partial clone* con objetos ausentes da "no calculable" con 0 conexiones (ADR-CKP-001, Validación 2 y 3).

### Validación 8: alcance del escaneo de secretos (DEP-CKP-3)

El escaneo de perfil, logs y captura del stream IPC sigue exigiendo **0 hallazgos**, salvo en las **respuestas a la consulta de diff**, que llevan por diseño contenido del repo que el humano pide desde la TUI (Q-CKP-8). Esas respuestas quedan fuera del escaneo con tres condiciones, que sí se verifican:

1. Solo van a la conexión CLI/TUI que pidió el diff; nunca al stream de difusión ni al MCP.
2. Nunca se persisten ni se registran: con el diff pedido varias veces sobre un repo con secretos plantados, el perfil y los logs siguen con 0 hallazgos.
3. La captura del stream de eventos y del resto de respuestas sigue con 0 hallazgos.

El arnés identifica las respuestas de diff por su id de petición para excluirlas. La forma de esa consulta es del contrato del canal: **pendiente, dueño: worker del canal (TS-GRP-004)**.

## Enmienda (2026-10-05, TD-GRP-001)

Decisión del orquestador (2026-10-05), validada por Arquitecto y security-expert. Origen: [TD-GRP-001](../../requirements/features/motor-local/technical-stories/TD-GRP-001-acl-windows.md) y su [Dev Spec](../../requirements/features/motor-local/dev-specs/TD-GRP-001-dev-spec.md). Sustituye el paréntesis "sin ACE de escritura para otros usuarios" de la validación del ejecutable en Windows. No cambia la regla de Unix. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| Regla de propietario y DACL para validar `git.exe` en Windows | § 4 | TD-GRP-001; DS-TD-GRP-001 D3 a D6 |
| Riesgos residuales y validación añadida | Esta sección | DS-TD-GRP-001 D8 y § 5 |

### Regla en Windows (SEC-10, M10)

- **Propietarios de confianza**, comparados por SID y nunca por nombre: el usuario actual, SYSTEM, Administrators y TrustedInstaller. `OWNER RIGHTS` cuenta como de confianza, porque el propietario ya lo es.
- **El archivo**: nadie más puede escribirlo, cambiar sus atributos, borrarlo ni cambiar su DACL o su propietario.
- **Su carpeta**: nadie más puede añadir archivos ni subcarpetas, borrar hijos, renombrarla ni cambiar su DACL. Así nadie deja una DLL junto al ejecutable (*DLL planting*).
- **Cada carpeta superior, hasta la raíz del volumen**: nadie más puede renombrarla, borrar hijos ni cambiar su DACL o su propietario. Añadir subcarpetas sí se permite, como concede `C:\` a *Authenticated Users*: una carpeta nueva no sustituye la ruta.
- **ACE ignorados en esta cadena**: los de denegar (solo quitan acceso) y los solo heredables (no se aplican al objeto).
- **Rechazo (fail-closed)**: un ACE de tipo desconocido, una ACL ilegible o NULL, un *reparse point* en la cadena, un volumen sin ACL persistentes y una ruta UNC.
- **Sin ubicaciones de confianza**: ninguna ruta se acepta por estar en `%ProgramFiles%`. Se verifica igual que cualquier otra.
- **Lanzador de Git for Windows**: si el candidato es `<raíz>\cmd\git.exe`, también se verifican el Git que lanza y su carpeta, cuando existen: `<raíz>\{mingw64,ucrt64,clangarm64,mingw32}\bin\git.exe` y `<raíz>\bin\git.exe`.

**Implementación**: crate `crates/winsys`, la única excepción a `forbid(unsafe_code)`. La excepción se registra en la enmienda de [ADR-GRP-002](./ADR-GRP-002-monorepo-nx.md) de la rama `feat/windows-requester-resolution`.

### Riesgos residuales

- El resto del árbol de la instalación de Git no se verifica.
- Los shims de Scoop viven en la carpeta del usuario y son de su confianza.
- En Unix no se comprueban las carpetas superiores del ejecutable. **Pendiente**.

### Validación añadida

En la Windows real se acepta `C:\Program Files\Git\cmd\git.exe`. Se rechazan:

- un `git.exe` con escritura para `Users`;
- su carpeta con `FILE_ADD_FILE` para `Users`;
- una carpeta superior con `FILE_DELETE_CHILD` para `Everyone`;
- un propietario `Users`;
- un *junction* en la cadena.
