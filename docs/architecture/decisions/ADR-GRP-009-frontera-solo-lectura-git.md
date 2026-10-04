---
id: ADR-GRP-009
title: Frontera de solo lectura e invocación del Git del sistema
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-012, ADR-GRD-001, TS-GRP-002, INF-GRP-001, CTX-GRP-001, BR-GRP-001]
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

- **Camino caliente con gitoxide** (`gix`), abierto en solo lectura: refs, HEAD, objetos, índice, estado del working tree, ahead/behind y metadatos de worktrees. No se usa ninguna API de escritura de `gix` (índice, refs, config, objetos, locks). Los archivos se abren con modos que permiten a otros procesos borrarlos y renombrarlos (en Windows, `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`).
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
| `merge-tree --write-tree` (escribe objetos en `.git/objects`) | Prohibido en el motor | Fuera de la allowlist (ver Consecuencias) |
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
- **Validación del ejecutable (SEC-10, M10)**: antes de invocarlo, todo candidato (también la ruta explícita) debe ser **absoluto, archivo regular, propiedad del usuario o de root y no escribible por grupo ni otros** (en Windows, sin ACE de escritura para otros usuarios). Un candidato que no cumple se descarta sin ejecutarlo y el motivo se reporta.
- **Selección**: el primer candidato válido que es ejecutable y responde a `git version` con 2.38 o superior. Si ninguno cumple, el motor pasa a "Esperando Git" (BR-WF-002) con el motivo (ausente o versión encontrada) para que la CLI y el Cockpit lo presenten.
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
- ⚠️ `merge-tree --write-tree`, la razón de Git 2.38 en NFR-07, escribe objetos en `.git/objects`. Queda prohibido en el motor. **Mitigación**: la predicción de conflictos (Cockpit) tendrá que escribir esos objetos en un almacén alternativo dentro del perfil (`GIT_OBJECT_DIRECTORY` con el repo como alternate) o pedir un ADR propio; no es alcance de motor-local.
- ⚠️ En Windows, gitoxide puede mapear packs en memoria y un mapeo abierto impide que el `git gc` del usuario los borre. **Mitigación**: handles de repo de vida corta, liberación al detectar un `gc` o `maintenance` en curso, y escenario de `gc` concurrente en INF-GRP-001.
- ⚠️ Un repo de otro propietario que Git rechaza por `safe.directory` no se puede observar sin tocar la config global (Q17). **Mitigación**: se reporta "no disponible" con el motivo y cómo resolverlo, sin escribir nada.

Nota de integración (Time Machine, ADR-TMC-002, aceptado el 2026-10-03): la frontera de solo lectura de este ADR es la del motor. `crates/git` aloja además una segunda lista cerrada, de escritura, que solo usa el módulo `timemachine` de `crates/core` (SEC-TMC-02, SEC-TMC-14). La comprobación estática de la Validación 5 se amplía: ni el observador del motor ni el ejecutor de operaciones de usuario pueden importar la capa de escritura de la Time Machine.

Nota de integración (Guardrails, ADR-GRD-001 § 7; Enmienda 2026-10-04): `crates/git` aloja una **segunda capa de escritura, separada**, la de Guardrails, análoga a la de la Time Machine. Es una lista cerrada de operaciones tipadas: escribir, restaurar o eliminar `core.hooksPath` en el `config` del directorio común; leer el valor efectivo de `core.hooksPath` por worktree; y crear, sustituir y borrar archivos listados solo dentro de `<git-common-dir>/gitraptor/`. Esas operaciones solo las alcanza el módulo `guardrails` de `crates/core` (visibilidad de módulo y frontera de Nx), con el Git CLI, argv fijo, sin shell, con el entorno por allowlist del § 3 y sin ejecutar hooks ni filtros. La capa tiene además **dos módulos de invocación autorizados, nombrados y tipados** (decisión del Arquitecto, 2026-10-04; ADR-GRD-001 § 7): uno **encadena el hook previo**, sin shell, y otro **ejecuta `git` para `raptor guard exec`**, con el argv validado y normalizado según ADR-GRD-007 § 3. `raptor hook` y `raptor guard exec` (`apps/cli`) los llaman y no tienen un `Command::new` propio. **No es una escritura del motor**: solo ocurre tras una instalación, desinstalación, actualización o adopción explícitas, que son comandos reservados (ADR-GRP-005 § 6). La frontera de solo lectura de este ADR sigue siendo la del motor, y INF-GRP-001 admite esas escrituras solo en sus escenarios de instalación. La comprobación estática de la Validación 5 se amplía también a esta capa.

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
5. **Allowlist**: el registro de argv del modo diagnóstico solo contiene subcomandos y opciones de la lista. Además, una comprobación estática en CI exige que `Command::new` aparezca solo en el módulo de invocación de `crates/git`. **Ampliada** (notas de integración de Time Machine y de Guardrails): el lanzamiento de procesos solo está en los módulos de invocación autorizados de `crates/git`, que la comprobación lista por nombre: lectura del motor, escritura de la Time Machine, escritura de Guardrails y los dos módulos de invocación de Guardrails (encadenado del hook previo y `git` de `raptor guard exec`). `raptor hook` y `raptor guard exec` no tienen un `Command::new` propio. Ni el observador del motor ni el ejecutor de operaciones de usuario pueden importar ninguna de las dos capas de escritura; las operaciones de escritura de Guardrails solo las importa el módulo `guardrails`, y sus dos módulos de invocación solo `raptor hook` y `raptor guard exec`.
6. **Resolución de Git**: tests con PATH mínimo; en macOS, un runner sin Command Line Tools comprueba que no se ejecuta `/usr/bin/git` (proceso no lanzado) y que el motor queda en "Esperando Git"; cambio de versión en caliente por debajo y por encima de 2.38.
7. **Cero ejecución de código configurable (SEC-09)**: repo canario con `filter.*.clean`, `diff.*.textconv`, `core.fsmonitor`, hooks y `gpg.program` apuntando a un script que deja un marcador; con archivos de stat sucio y el motor observando, el marcador nunca aparece. **Auditoría dinámica de `exec`** en INF-GRP-001 (eslogger en macOS, ETW en Windows, strace en Linux): todo proceso hijo del motor pertenece a la allowlist y `gix` no lanza `git` (M1).
8. **Secretos (SEC-05)**: suite con secretos plantados (`.env`, token en la URL del remoto, `http.extraHeader`) y gitleaks/trufflehog sobre perfil, logs y captura del stream IPC: 0 hallazgos.
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

**Pendiente de motor-local** (anotado el 2026-10-04, no se decide en esta enmienda): la regla de la Validación 5 ("`Command::new` solo en los módulos de invocación autorizados de `crates/git`") choca con lanzamientos de procesos que ya existían en ADR-GRP-005 § 3: el arranque del daemon bajo demanda desde la biblioteca cliente y las peticiones al gestor de servicios (`launchctl kickstart`, `systemctl --user start`). Es una tensión previa de motor-local; falta decidir en qué módulo autorizado viven esos lanzamientos.
