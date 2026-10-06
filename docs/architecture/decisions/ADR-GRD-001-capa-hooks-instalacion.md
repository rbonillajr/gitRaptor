---
id: ADR-GRD-001
title: Capa de hooks — instalación, encadenado, desinstalación recuperable y cobertura de worktrees
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-06
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRD-002, ADR-GRD-003, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, CTX-GRD-001, BR-GRD-001, SPIKE-GRD-001]
tags: [guardrails, hooks-git, core-hookspath, dispatcher, encadenado, instalacion-transaccional, nfr-01, nfr-12, worktrees, husky, lefthook, pre-commit, fail-closed, integridad, actualizacion-binario, spike-grd-001, dispatcher-nativo, reftable]
---

# ADR-GRD-001 — Capa de hooks: instalación, encadenado, desinstalación recuperable y cobertura de worktrees

> **Estado**: aceptado por Rene Bonilla el 2026-10-04. Enmendado el 2026-10-04 con los resultados de SPIKE-GRD-001 en macOS (ver "Enmienda (2026-10-04, SPIKE-GRD-001)") y el 2026-10-05 por US-GRD-001 (forma del dispatcher nativo y coste en Windows, ver "Enmienda (2026-10-05, US-GRD-001)"); la matriz del spike en Linux y la validación funcional en Windows siguen pendientes.

## Contexto

Guardrails es la única feature que instala hooks de Git (BRD BR-12; Q22 de motor-local). La instalación es una modificación operativa del repo del usuario y tiene que cumplir:

- **Permiso explícito** (BR-AUTH-002, Q-GRD-3): solo el humano concede. Un permiso vale para un repo y no se vuelve a preguntar tras una denegación. El mecanismo de autorización está en ADR-GRD-007.
- **Cero pérdida** (BR-CONS-005, NFR-01): los hooks previos conservan contenido y efecto, la desinstalación deja las rutas operativas exactamente como estaban y una instalación interrumpida queda completa o idéntica a la anterior (NFR-12).
- **Solo dentro del repo** (Q17 de motor-local): nada en la configuración global ni de sistema de Git, ni en plantillas ni en otros repos.
- **Hooks previos y otros gestores** (BR-EDGE-002, Q-GRD-4): detectar, informar y encadenar solo con permiso. Si no se puede encadenar sin alterarlos, no se instala nada.
- **Todos los worktrees** (S-GRD-6), los actuales y los futuros. Su viabilidad técnica es responsabilidad del Arquitecto.
- **Binario ausente** (decisión 4 de Rene Bonilla, 2026-10-04): comportamiento **mixto**. Fail-closed con mensaje de recuperación solo en los hooks que gobiernan operaciones de riesgo (`pre-push`, `pre-rebase` y borrado de refs vía `reference-transaction`); fail-open con aviso en el resto.

Restricciones heredadas:

- El motor es de solo lectura (ADR-GRP-009, aceptado) y el daemon es el único escritor del perfil (ADR-GRP-005 y ADR-GRP-006, aceptados).
- La Time Machine ya fijó un precedente: una capa de escritura propia, separada de la de lectura, en `crates/git` (ADR-TMC-002 § 1, rama time-machine, en `main`).
- Stack (ADR-GRP-001, ADR-GRP-002): Rust, `crates/{core,policy,git,api}`, binarios `raptor` y `raptor-mcp`, y Git ≥ 2.38 (NFR-07).

> **Constitución**: no hay `architecture-constitution.md` en la cascada del repo. Las restricciones se toman de `AGENTS.md` y de ADR-GRP-001..004 (aceptados). Fuente: inline. Se formaliza con `/aadd-architect --init-constitution`.

## Decisión

**Una carpeta de dispatchers propia dentro del directorio Git común, activada con `core.hooksPath` (ruta absoluta) en la configuración local del repo. Cada dispatcher es una lista de constantes que invoca al binario instalado; la evaluación corre con un entorno por allowlist y, si permite, el binario encadena el hook previo sin moverlo ni editarlo. La instalación es una transacción con un único punto de commit, la ejecuta el daemon tras un comando reservado, y la referencia de integridad vive en el perfil.**

### 1. Qué se escribe y dónde

| Elemento | Ubicación | Contenido |
|---|---|---|
| Carpeta de Guardrails | `<git-common-dir>/gitraptor/` | Creada por Guardrails; nada más la usa |
| Dispatchers | `<git-common-dir>/gitraptor/hooks/<nombre-del-hook>` | Script POSIX `sh` generado, solo con constantes (§ 2). También corre con el `sh` de Git for Windows |
| Manifiesto | `<git-common-dir>/gitraptor/manifest.json` | **Solo para la recuperación manual** y como evidencia de respaldo (ADR-GRD-005): versión de plantilla, valor previo de `core.hooksPath` **y su nivel** (worktree, local, global, sistema o ninguno), ruta del binario, lista de hooks previos encadenados, fecha. **No es autoritativo**: un proceso del mismo usuario lo puede editar |
| Clave de activación | `core.hooksPath` en el `config` del directorio común (nivel local), con la ruta **absoluta** de `gitraptor/hooks` | Es el único cambio en un archivo que ya existía |

- **Referencia de integridad autoritativa** (H-04): vive en el **diario de instalación del perfil**, que solo escribe el daemon y que se guarda en el **almacén por repo** (ADR-GRP-006 § 4; decisión del Arquitecto, 2026-10-04). Guarda:
  - el hash de cada dispatcher;
  - la ruta estable del binario, su destino canónico y su firma o huella (§ 8);
  - el identificador del directorio común;
  - `dev/inode` de la carpeta y del `config`.

  La comprobación de estado (ADR-GRD-005) usa el diario. El manifiesto no se usa para eso.
- **No se toca nada más**: ni `.git/hooks`, ni los hooks del usuario, ni el directorio de otro gestor, ni la configuración global o de sistema, ni las plantillas.
- **Ruta absoluta**: Git resuelve una ruta relativa de `core.hooksPath` contra la raíz de cada worktree, así que solo una ruta absoluta apunta a la misma carpeta desde todos.
- **Permisos**:
  - **macOS y Linux**: carpeta 0700; dispatchers 0700, ejecutables y escribibles solo por el usuario.
  - **Windows** (M-07): DACL sin ninguna ACE de escritura para `Everyone`, `Users` ni `Authenticated Users`. Solo el SID del usuario, más `SYSTEM` y `Administrators` heredados.

### 2. Dispatchers (M-04)

- **Solo constantes** (hallazgo 1 de la ronda 2 del Judge): todo lo que el hook necesita para localizar al daemon y su propio repo va **escrito como constante** en el dispatcher al instalar. `raptor hook` **no lee el diario ni el perfil**, ni deriva nada del entorno, para resolverlo.

  | Constante | Para qué |
  |---|---|
  | `<raptor>` | Ruta estable del binario instalado (§ 8) |
  | Nombre del hook | Cuál de los hooks de Git es |
  | Id del directorio común | El repo al que pertenece el dispatcher (M-02, SEC-GRD-19) |
  | Ruta del canal del daemon | Socket o named pipe fijado al instalar (H-03, SEC-GRD-16) |
  | Id de la instancia del perfil | Identificador opaco que el daemon genera al crear el perfil y presenta en el handshake (hallazgo 3; ADR-GRD-003 § 4) |
  | Ruta del directorio de estado del perfil | Solo para el spool y la instantánea del modo degradado (ADR-GRD-003 § 4) |
  | Valor previo de `core.hooksPath`, tal cual | Hook previo que se encadena (abajo) |

  - **Llamada**: cada dispatcher ejecuta `'<raptor>' hook` con esas constantes como argumentos y `"$@"` al final.
  - **Charset seguro**: las constantes van entre comillas simples y solo con caracteres imprimibles ASCII distintos de la comilla simple. Los bytes no ASCII de una ruta (p. ej. `C:/Users/José`) se emiten como escapes octales de `printf`, que también son constantes.
  - **Valor no representable**: si alguna constante contiene comilla simple, salto de línea u otro carácter de control, **no se instala** y se explica el motivo. ⚠️ **ASSUMPTION**: SPIKE-GRD-001 confirma la representación en los tres SO.
- **Evaluación** con entorno por allowlist:
  - **Qué entorno recibe**: el dispatcher lanza la evaluación con un entorno construido desde cero, con solo cuatro cosas: la variable del token de excepción (ADR-GRD-007); `GIT_INDEX_FILE` si Git la fijó (índice temporal de `commit <rutas>`); `GIT_DIR`, que el daemon solo usa para contrastarlo con el id del directorio común; y en Windows las variables de sistema imprescindibles para arrancar un proceso (`SystemRoot`).
  - **Sin `HOME`**: con todas las rutas como constantes, `raptor hook` no necesita `HOME`, y quitarlo elimina un vector de redirección. Tampoco recibe PATH, `XDG_*`, `GIT_CONFIG_*`, `LD_*` ni `DYLD_*`.
  - **Qué hace**: la evaluación valida y normaliza la entrada (ADR-GRD-002 § 4) y consulta al daemon (ADR-GRD-003).
- **Encadenado sin shell**: si la evaluación permite, el **binario `raptor`**, no el `sh`, ejecuta el hook previo. Lo hace con los mismos argumentos y la misma entrada estándar, y con el **entorno original de Git menos la variable del token**, como lo habría ejecutado Git (NFR-07). Devuelve el código de salida del hook previo.
  - **Módulo de invocación** (decisión del Arquitecto, 2026-10-04): `raptor hook` no lanza el hook previo con un `Command::new` propio. Llama al módulo de invocación autorizado de la capa de escritura de Guardrails que **encadena el hook previo** (§ 7), sin shell.
  - **Hook previo**: sale de la constante del valor previo, nunca del diario.
    - Si no había valor: `<git-common-dir>/hooks/<nombre>`, también escrito como constante.
    - Si es absoluto: esa carpeta más el nombre.
    - Si es relativo (p. ej. `.husky/_`): **relativo al directorio actual**. Git ejecuta los hooks con el directorio actual en la raíz del worktree (en un repo bare, en `$GIT_DIR`), así que se resuelve igual que lo resolvería Git.
  - **Mismo criterio que Git**: solo se encadena un hook previo que exista y sea ejecutable.
  - **Cómo pasa el veredicto**: lo transmite un canal entre los dos procesos que fija el dispatcher. Que el encadenado corra con el entorno original no da ningún poder nuevo: es el mismo entorno con el que Git ejecutaría el hook previo. El mecanismo concreto (p. ej. una tubería con un veredicto enmarcado) lo fija la Dev Spec de US-GRD-001.
- **Si la evaluación deniega**, el hook previo no se ejecuta, porque la operación no va a ocurrir.
- **Conjunto de dispatchers** (Enmienda 2026-10-04, SPIKE-GRD-001 § 7 y § 8): **solo los necesarios**, porque cada dispatcher sin función cuesta un proceso en cada operación:
  - **Obligatorios** (US-GRD-001): `pre-push`, `reference-transaction` y `pre-rebase`.
  - Los de los hooks que tengan una **política activa** (p. ej. `pre-commit` y `commit-msg` para US-GRD-008 y US-GRD-009).
  - Los de los **hooks previos** que haya que encadenar. Con un valor previo relativo, la unión de los hooks previos de todos los worktrees. ⚠️ **ASSUMPTION** (por verificar): husky 9 genera en `.husky/_` todos los nombres de hook, así que en un repo con husky el conjunto vuelve a ser el completo; se declara.
  - `push-to-checkout`, `proc-receive` y `post-index-change` **solo si el directorio previo ya los tenía** (confirmado: el primero deja sin actualizar el working tree del receptor, el segundo rompe los pushes con `receive.procReceiveRefs` y el tercero corre en cada `git status`).
  - **Regeneración** como la misma instalación, con auditoría (como el § 8): cuando cambia la política efectiva o aparece o desaparece un hook previo. Archivo por archivo dentro de `gitraptor/` (creación, renombrado atómico o borrado), sin tocar `core.hooksPath`, con los permisos del § 1, comprobando el `dev/inode` antes de escribir, con un lock por repo frente a la comprobación H3 de ADR-GRD-005, y cada hash nuevo en el diario. Renombrar la carpeta entera no sirve: el renombrado no es atómico sobre un directorio que no está vacío.
  - **Orden**: al activar una política, primero el dispatcher y después la política vigente; al desactivarla, al revés. Así no hay ventana sin dispatcher para una política activa.
  - **Ventana declarada**: un hook previo añadido después de instalar no corre hasta la regeneración (Git no ejecuta un hook sin dispatcher). Se detecta con el diagnóstico `hook-previo-no-encadenado` en la comprobación de estado y al arrancar el daemon (ADR-GRD-005).
- **`reference-transaction`** (L-01; Enmienda 2026-10-04):
  - **Sin hook previo**: su primera línea sale con 0 en **cualquier estado distinto de `prepared`** (`preparing` desde Git 2.54, `committed`, `aborted` y cualquier estado futuro), sin lanzar ningún proceso.
  - **Con hook previo**: en esos estados solo encadena. El binario solo evalúa en el estado `prepared`.
- **Dispatcher nativo** (Enmienda 2026-10-04; SPIKE-GRD-001 § 8): un dispatcher `sh` cuesta en macOS unos 8–11 ms por invocación y un binario nativo, 1–4 ms. Con Git ≥ 2.54 cada transacción lanza tres procesos y un commit lanza 7 solo en `reference-transaction`, así que el dispatcher nativo es **necesario para `reference-transaction` desde Git 2.54 en los tres SO**, y para todo el conjunto en Windows si la medición lo confirma. Conserva la regla de "solo constantes" (M-04), la firma o la huella del § 8 y la comprobación del canal (SEC-GRD-16, SEC-GRD-19), y pasa revisión de seguridad. Su forma concreta (p. ej. un stub con las constantes incrustadas, generado al instalar) y el respaldo cuando falta `raptor` (§ 3) los fija la Dev Spec de US-GRD-001 con el banco de INF-GRD-001. ⚠️ **ASSUMPTION**: ≤ 5 ms p95 por invocación que no evalúa (ADR-GRD-002 § 5).
- **macOS** (I-02): el binario se firma con hardened runtime y sin el entitlement `allow-dyld-environment-variables`. Así `DYLD_INSERT_LIBRARIES` no afecta a `raptor hook`.

### 3. Binario ausente o fallo interno (decisión 4; J2)

| Situación | `pre-push`, `pre-rebase` | `reference-transaction` en `prepared` | Resto (incluidos `pre-commit`, `commit-msg` y las actualizaciones que crean o mueven `refs/heads/*`) |
|---|---|---|---|
| `raptor` ausente o no ejecutable (el `sh` lo comprueba con constantes) | **Fail-closed** con mensaje de recuperación | **Fail-closed solo si alguna línea borra** una ref `refs/heads/*` (valor nuevo igual a ceros; el `sh` lo comprueba leyendo la entrada). Si no, pasa con aviso | **Fail-open** con el aviso "protección inactiva" y el hook previo encadenado por el `sh` |
| `raptor hook` falla con un error interno | Fail-closed | Fail-closed solo para el **borrado** de `refs/heads/*`. Un commit, un `fetch` o un tag pasan con aviso | Fail-open con aviso y encadenado |
| **Señales de ataque**: canal no auténtico (H-03, SEC-GRD-16), directorio común distinto del fijado (M-02, SEC-GRD-19), tope de entrada superado (M-01) | Deny | Deny en refs gobernadas | Deny en refs gobernadas. No es un error interno: nunca cae a la vía rápida |

- **Código fijo del `sh` cuando falta el binario** (hallazgo 2 de la ronda 2 del Judge): es el mismo texto para todos los repos; solo cambian las constantes del § 2. No usa `$(…)`, `eval` ni lanza ningún programa salvo el hook previo.
  1. **Comprobación del binario**: `[ -x '<raptor>' ]`. Si existe, ejecuta el binario (§ 2) y el resto no se aplica.
  2. **`pre-push` y `pre-rebase`**: escribe en la salida de error el mensaje de recuperación (constante) y sale con 1.
  3. **`reference-transaction`**:
     - En cualquier estado distinto de `prepared` encadena con la entrada estándar heredada (Enmienda 2026-10-04).
     - En `prepared` lee **toda** la entrada línea a línea con `while IFS=' ' read -r viejo nuevo ref` y **acumula cada línea en una variable** (`lineas`) antes de decidir. Si alguna línea tiene un `nuevo` igual a una de las dos constantes de ceros (SHA-1 o SHA-256) y una `ref` que empieza por `refs/heads/`, sale con 1 y el mensaje, sin encadenar, **salvo que sea el *prune* de `pack-refs`** de ADR-GRD-002 § 4 (Enmienda 2026-10-04): `viejo` distinto de cero, el archivo suelto `<common>/<ref>` contiene `viejo` y `packed-refs` contiene exactamente `viejo ref`. El `sh` lo comprueba leyendo los dos archivos con `read` y redirección, sin lanzar programas. Sin esta excepción, `gc` fallaría mientras falte el binario (SPIKE-GRD-001 D14).
     - Si ninguna línea es un borrado, escribe el aviso "protección inactiva" en la salida de error, **reenvía explícitamente** la variable acumulada al hook previo con `printf '%s'` por una tubería y encadena.
     - Una línea que no se puede partir en tres campos se trata como borrado (fail-closed).
  4. **Resto de hooks**: escribe el aviso "protección inactiva" y hace `exec` del hook previo (constante del § 2) con `"$@"` y la entrada estándar heredada, sin leerla. Si el hook previo no existe o no es ejecutable, sale con 0.
  5. **Hook previo con valor relativo**: el `sh` usa la constante tal cual, relativa al directorio actual, por el mismo contrato de Git que el § 2.
- **El mensaje de recuperación** dice que GitRaptor no está disponible y remite al procedimiento documentado: reinstalar `raptor` o restaurar a mano con los valores del manifiesto. No incluye comandos para desactivar la protección ni vías de excepción (SEC-GRD-06).
- **Daemon inalcanzable** (con el binario presente): lo trata ADR-GRD-003 § 4 (evaluación degradada).

### 4. Instalación y desinstalación transaccionales (NFR-12; M-03)

Las ejecuta el **daemon** (módulo `guardrails` de `crates/core`) tras el comando reservado de ADR-GRD-007. Cada paso se anota antes y después en el **diario de instalación del perfil**, escrito solo por el daemon (ADR-GRP-006).

**Reglas de archivo en `.git`**:

- Toda operación se hace relativa a un descriptor del directorio común, sin seguir enlaces (`openat` con `O_NOFOLLOW`, o su equivalente en Windows).
- El temporal lleva un nombre aleatorio y se crea en exclusiva.
- El daemon guarda en el diario el `dev/inode` de la carpeta y lo verifica antes de cualquier escritura o borrado posterior.
- **El borrado** solo alcanza los archivos que lista el diario y después el directorio, si quedó vacío. Nunca es recursivo.

**Instalación**:

1. **Comprobaciones previas, sin escribir nada**: el repo está observado (Q-GRD-15); la cobertura de worktrees se puede garantizar (§ 5); los hooks previos se pueden encadenar (§ 6); el binario es el instalado y está firmado o con la huella fijada (§ 8); la ruta es representable (§ 2).
   - **Si falla una**: no se instala nada, se explica el motivo y el intento queda guardado con su causa. Si la causa es el encadenado, el diagnóstico es `encadenado-imposible` (J6, ADR-GRD-005).
2. **Carpeta**: la carpeta completa se escribe en el temporal, se sincroniza y se renombra de forma atómica a `gitraptor/`. Si ya existe una carpeta `gitraptor/` que no figura en el diario como instalación vigente, se trata como `instalacion-huerfana` (ADR-GRD-005 § 1): no se toca y se ofrece adoptarla o retirarla.
3. **Punto de commit**: escribir `core.hooksPath` en el `config` del directorio común con el Git CLI, argv fijo y `--file` explícito. Git lo escribe con su lock y un renombrado atómico.
   - **Después**: se verifica que el `config` es un archivo regular, no un enlace, y su `dev/inode` se anota en el diario. La desinstalación lo comprueba antes de tocarlo.
4. **Verificación**: el valor **efectivo** de `core.hooksPath` en cada worktree existente apunta a la carpeta. Si no es así, se revierte en orden inverso y se informa.
5. **Cierre**: el diario queda en `confirmada`, con la referencia de integridad. La instalación va a la auditoría append-only de ADR-GRP-013 y al registro de decisiones (ADR-GRD-006).

**Desinstalación**, en orden inverso:

1. **Clave**: si el valor local actual es el de Guardrails, se restaura el valor previo **solo si era de nivel local**, o se elimina la clave si no lo era.
   - **Criterio semántico** (Enmienda 2026-10-04; SPIKE-GRD-001 § 5.1): la restauración con `git config` deja el mismo valor efectivo, el mismo nivel y las demás entradas sin cambios, pero **no garantiza la identidad byte a byte**: Git reescribe una línea escrita a mano (formato y comentario) y añade el salto de línea final que faltaba. Esas diferencias de formato se declaran (NFR-GRD-01, BR-CONS-005). Se descarta guardar y restaurar la línea original a mano: editar el texto del `config` fuera de Git es más arriesgado que la diferencia de formato.
   - Si otro gestor la cambió después, **no se toca**: la protección ya estaba inactiva y se conserva el cambio del tercero.
2. **Carpeta**: se borran los archivos del diario y después el directorio vacío.

**Recuperación al arrancar el daemon**: por cada entrada del diario sin estado final:

- **Instalación**:
  - La clave no es la de Guardrails: se borran la carpeta y el temporal, y el repo queda idéntico al anterior.
  - La clave es la de Guardrails: se verifica (paso 4). Si la verificación pasa, la instalación queda `confirmada`; si no, se revierte.
- **Desinstalación**:
  - La clave ya está restaurada: se borra la carpeta.
  - La clave sigue siendo la de Guardrails: la protección sigue completa y la desinstalación se marca `no completada` y se avisa.

### 5. Cobertura de worktrees (S-GRD-6)

- **Por qué funciona**: los worktrees enlazados comparten el directorio común y su `config`, así que la clave local alcanza a los actuales y a los que se creen después. Los hooks también se resuelven desde el directorio común.
- **No se instala, y se explica el motivo**, si la cobertura no se puede garantizar:
  - `extensions.worktreeConfig` activo con `core.hooksPath` en el `config.worktree` de algún worktree.
  - Un `include` o `includeIf` en el nivel local o de worktree que defina `core.hooksPath`.
  - Cualquier `includeIf "onbranch:…"` en el nivel local o de worktree.
- **Configuración global o de sistema** con `core.hooksPath`: no impide instalar, porque el nivel local gana. Se guarda como valor previo para encadenarlo.
- **Cómo se comprueba** (Enmienda 2026-10-04; SPIKE-GRD-001 § 5.2): `git config --show-scope --show-origin --get core.hooksPath` en cada worktree, al instalar y en cada comprobación de estado (ADR-GRD-005). Basta para detectar todos los casos anteriores y una clave relativa.
- **Repos reftable** (Enmienda 2026-10-04): **se instalan**. El renombrado de la rama base no pasa por ningún hook y se declara en la lista del repo (ADR-GRD-002 § 3, Enmienda); la explicación del permiso lo dice.
- **Después de instalar**, cualquier sobrescritura posterior es una pérdida de protección, y la detecta ADR-GRD-005.

### 6. Gestores existentes

Comportamiento verificado en macOS por SPIKE-GRD-001 (husky 9.1.7, lefthook 2.1.16, pre-commit 4.6.2; Enmienda 2026-10-04). Ningún gestor probado obliga a "no se instala" por el encadenado:

| Caso | Al instalar | Si el gestor se reinstala después |
|---|---|---|
| Hooks propios en `.git/hooks` | Se encadenan desde `<git-common-dir>/hooks` | — |
| husky (`core.hooksPath` local relativo, p. ej. `.husky/_`) | Se guarda el valor y su nivel. Se encadena resolviendo la ruta por worktree. **husky no corre en los worktrees enlazados ni sin Guardrails** (`.husky/_` está ignorado y no existe allí): el encadenado relativo lo conserva y no es un fallo de cobertura | Vuelve a escribir la clave en silencio (`npm install` con `prepare`): la protección pasa a **inactiva** con aviso (US-GRD-004) |
| lefthook 2.x (escribe en `.git/hooks`) | Se encadenan sus scripts desde `.git/hooks` | `lefthook install` **se niega** con `core.hooksPath` definido y la protección sigue intacta (también se salta su resincronización automática). Con `--force` escribe en la carpeta de Guardrails y renombra el dispatcher a `.old`: el hash no cuadra con el diario y la protección pasa a **inactiva** con aviso. Con `--reset-hooks-path` borra la clave: **inactiva** con aviso |
| pre-commit (framework) en `.git/hooks` | Se encadena | Se niega a instalar con `core.hooksPath` definido: la protección sigue intacta |
| Un hook previo que no se puede encadenar sin alterarlo | No se instala nada; el intento queda guardado con `encadenado-imposible` (J6) | — |

### 7. Quién escribe y cómo convive con la frontera del motor

- **Módulo `guardrails` en `crates/core`**, alojado en el daemon. Los clientes piden instalar o desinstalar por el canal y nunca escriben.
- **Capa de escritura de Guardrails en `crates/git`**, separada de la capa de lectura del motor (ADR-GRP-009) y de la de la Time Machine (ADR-TMC-002). Tiene una lista cerrada de operaciones tipadas:
  - Escribir, restaurar o eliminar `core.hooksPath` en el `config` del directorio común.
  - Leer el valor efectivo de `core.hooksPath` por worktree.
  - Crear, sustituir y borrar archivos listados, **solo** dentro de `<git-common-dir>/gitraptor/` y con las reglas del § 4.

  Estas operaciones solo las alcanza el módulo `guardrails` (visibilidad de módulo, frontera de Nx y la comprobación estática de CI de ADR-GRP-009 Validación 5). Usan el Git CLI con argv fijo, sin shell, con un entorno por allowlist (ADR-GRP-009 § 3) y sin ejecutar hooks ni filtros.
- **Dos módulos de invocación autorizados, nombrados y tipados** en esa capa (decisión del Arquitecto, 2026-10-04):
  - **Encadenado del hook previo**: ejecuta el hook previo sin shell, con los argumentos, la entrada estándar y el entorno del § 2.
  - **Ejecución de `git` para `raptor guard exec`**: ejecuta `git` con el argv validado y normalizado según ADR-GRD-007 § 3, por la ruta validada (ADR-GRP-009 § 4) y con los `-c` que neutralizan ejecutables.

  Solo los alcanzan `raptor hook` y `raptor guard exec` (`apps/cli`), que no tienen un `Command::new` propio. El `git` de `raptor guard exec` sí ejecuta los hooks gobernados (así presenta el token) y lo que ADR-GRD-007 § 2 no neutraliza. La comprobación estática de CI de ADR-GRP-009 (Validación 5) lista estos dos módulos junto a los demás autorizados.

- **El motor sigue siendo de solo lectura**: estas escrituras no son del motor y solo ocurren tras una instalación, desinstalación, actualización (§ 8) o adopción explícita.
- **Lo que necesita INF-GRP-001**: una excepción por escenario, como la de PQ-1. **Aplicada (2026-10-04)** en INF-GRP-001 y en la nota de integración de ADR-GRP-009 (tabla de [non-functional-guardrails.md](../non-functional-guardrails.md)).

### 8. Actualización y movimiento del binario (J4; H-04)

- **Ruta estable**: el dispatcher usa una ruta estable del binario instalado. Puede ser un enlace estable, como el de Homebrew en `bin/`, que apunta a una carpeta con versión, o la ruta fija del instalador.
  - **Validación**: el destino canónico de esa ruta tiene que ser **el mismo binario que el daemon** (regla SEC-14 de motor-local). Se verifica por su firma (codesign en macOS, Authenticode en Windows); en Linux, por la **huella fijada** en el diario al instalar o actualizar.
  - **Rechazos**: la caché de npx y las carpetas temporales se rechazan.
- **Actualización** (nueva versión en la misma ruta estable, o destino nuevo tras una actualización del gestor de paquetes): el daemon de la nueva versión comprueba la firma, o la huella publicada en Linux, y **refresca la referencia de integridad del diario como la misma instalación**. Eso queda en la auditoría y **no pide permiso nuevo**, porque no cambia qué se instala ni dónde.
- **Cambio de constantes**: si cambia una constante del § 2 (ruta estable, ruta del canal o id de instancia tras adoptar una instalación huérfana, ADR-GRD-005 § 1), el daemon regenera los dispatchers con la transacción del § 4, como la misma instalación y con auditoría.
- **Plantilla de dispatchers versionada**:
  - **Compatible**: el binario admite la versión de plantilla actual y la anterior.
  - **Incompatible**: si la versión nueva necesita otra plantilla, el daemon regenera los dispatchers con la misma transacción del § 4, como la misma instalación y con auditoría. Solo cambia el contenido de la carpeta.
- **Ruta estable rota o destino sin firma ni huella válida**: el estado pasa a `binario-ausente` o `binario-no-valido` (ADR-GRD-005) y se aplica el § 3. Una ruta estable nueva (otro método de instalación) es una **instalación nueva con permiso**.

### 9. Repo movido de carpeta

- **Qué pasa**: la ruta absoluta queda rota y Git no ejecuta ningún hook, sin dar ningún error.
- **Cómo se detecta**: ADR-GRD-005 lo marca como inactivo (`repo-movido`).
- **Cómo se repara**: con una instalación nueva, que pide permiso nuevo (BR-AUTH-002, punto 4).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Escribir los hooks de Guardrails en `.git/hooks` (mover o renombrar los del usuario) | Altera los hooks del usuario y los gestores que escriben ahí (BR-CONS-005, R-GRD-5) |
| Añadir una línea de Guardrails dentro de los scripts existentes | Edita contenido del usuario. Falla con gestores que regeneran sus scripts |
| Dispatchers en el perfil con `core.hooksPath` apuntando allí | Fuera del repo (Q17). Además, borrar el perfil dejaría el repo sin hooks, sin aviso |
| `core.hooksPath` global o plantillas de Git | Fuera del repo y alcanza otros repos (Q17) |
| Hooks definidos por configuración (varios por evento) | No están disponibles en el mínimo soportado, Git 2.38 (NFR-07) |
| Dispatcher que interpola rutas y encadena en `sh` | Inyección por la ruta y por el entorno del agente (M-04). Se sustituye por constantes y encadenado en Rust |
| Manifiesto en el repo como referencia de integridad | Lo puede editar el agente (H-04). Queda solo para la recuperación manual |
| Pedir permiso nuevo en cada actualización del binario | Cada `brew upgrade` volvería a preguntar, y eso empuja a desinstalar. No cambia qué se instala (J4) |
| Instalar sin transacción | Una interrupción deja la clave apuntando a una carpeta incompleta (NFR-12) |

## Consecuencias

- ✅ Los hooks del usuario y de otros gestores no se mueven ni se editan. La desinstalación restaura la clave previa solo si era local y borra solo lo que lista el diario.
- ✅ Un único punto de commit, atómico, con recuperación desde el diario del perfil.
- ✅ Cubre los worktrees actuales y futuros por construcción, y se niega a instalar cuando no puede garantizarlo.
- ✅ El agente no controla qué binario se ejecuta ni su entorno de evaluación: constantes, binario firmado o con huella, y allowlist.
- ✅ Un error interno nunca bloquea los commits ni los `fetch` (J2).
- ⚠️ **Los hooks no son una frontera de seguridad frente a un proceso del mismo usuario**: `-c core.hooksPath=…`, `GIT_CONFIG_*` o editar `.git/config` los desactivan. La detección persistente es de ADR-GRD-005; lo que ocurre en un solo comando se declara (ADR-GRD-002, R-GRD-1 aceptado).
- ⚠️ **Coste**: dos procesos (`sh` y `raptor`) en cada hook gobernado. **Mitigación**: la vía rápida y el presupuesto NFR-GRD-04, medidos en SPIKE-GRD-001. En Windows el arranque de `sh` es el riesgo principal. **Enmienda 2026-10-04**: medido en macOS, el coste lo pone el número de procesos por comando; se mitiga con el conjunto mínimo de dispatchers y el dispatcher nativo de `reference-transaction` (§ 2), y las operaciones masivas tienen un coste lineal declarado (ADR-GRD-002 § 5).
- ⚠️ **Fail-closed con el binario ausente** bloquea `push`, `rebase` y el borrado de ramas hasta reinstalar o restaurar a mano. Es la decisión 4 de Rene.
- ⚠️ **Rutas con caracteres no representables**: no se instala. Lo explica el motivo y lo mide SPIKE-GRD-001.
- ✅ **Enmiendas de motor-local aplicadas (2026-10-04)**: ADR-GRP-009 (segunda capa de escritura y Validación 5) e INF-GRP-001 (excepción por escenario). Ver la tabla de [non-functional-guardrails.md](../non-functional-guardrails.md).

## Validación

Siempre con repos y perfiles temporales (INF-GRD-001), nunca con este repo.

1. **Huella**: instalar y desinstalar sin hooks, con hooks propios, con husky, con lefthook y con pre-commit. Tras desinstalar, la huella es idéntica a la de antes de instalar. La configuración global, la de sistema y un repo vecino no cambian.
2. **Encadenado**: un hook previo de linter rechaza un commit con error igual que antes; su contenido no cambia; el force-push sigue denegado (US-GRD-002). El hook previo recibe el entorno original menos el token.
3. **Interrupción**: matar el proceso antes y después de cada paso del § 4, y durante el renombrado y la escritura de la clave. Al relanzar, el repo queda completo o idéntico al anterior (US-GRD-003).
4. **Worktrees**: un worktree creado tras instalar queda protegido. Con `worktreeConfig` o `includeIf "onbranch:"`, no se instala y se explica.
5. **Binario ausente**: `pre-push` y `pre-rebase` fallan con el mensaje de recuperación; borrar una rama se rechaza; un `commit` y un `fetch` pasan con aviso.
6. **Error interno** (J2): un fallo provocado en `raptor hook` deja pasar con aviso un `commit` y un `fetch`, y rechaza el borrado de una rama.
7. **Constantes y código fijo** (M-04; ronda 2 del Judge):
   - **Instalación**: una ruta con `$(…)` se instala como constante literal; una con comilla simple o salto de línea no se instala.
   - **Contenido del dispatcher generado**: aparte de las constantes, solo `"$@"`, `$1`, las variables de `read` y la asignación que acumula las líneas leídas (`lineas="$lineas$linea<salto>"`, sin expansión de comandos). Esas variables se usan únicamente en comparaciones y en `printf '%s'`.
   - **Aviso sin binario**: un `reference-transaction` sin borrados escribe el aviso "protección inactiva".
   - **Entorno**: con `PATH`, `HOME`, `XDG_*`, `LD_PRELOAD` o `GIT_CONFIG_*` hostiles, la evaluación no los ve.
   - **Hook previo relativo**: con husky (valor relativo), sin el binario, un commit pasa con aviso y ejecuta el hook de husky de la raíz del worktree.
   - **Reenvío de la entrada**: sin el binario, un `reference-transaction` que no borra ramas reenvía al hook previo exactamente las líneas recibidas. Uno que borra `refs/heads/main` sale con 1. Una línea malformada sale con 1.
8. **Archivos en `.git`** (M-03): `gitraptor/` sustituida por un enlace, o el temporal pre-creado como enlace → se aborta sin escribir fuera. Un archivo ajeno añadido a la carpeta no se borra al desinstalar. Un `config` sustituido por un enlace tras la escritura → se detecta y no se toca.
9. **Integridad** (H-04): editar el manifiesto no cambia el estado; editar un dispatcher sí (contra el diario). Un binario sin firma, o con otra huella, en la ruta estable → `binario-no-valido`.
10. **Actualización** (J4): simular `brew upgrade` (nuevo destino del enlace estable, firmado) → la referencia se refresca sin pedir permiso y queda en la auditoría. Una plantilla nueva regenera los dispatchers dentro de la transacción y es resistente a interrupciones.
11. **Windows** (M-07): la DACL de la carpeta y de los dispatchers no tiene ACE de escritura para `Everyone`, `Users` ni `Authenticated Users`.
12. **Frontera**: la comprobación estática de CI confirma que solo el módulo `guardrails` alcanza la capa de escritura de Guardrails.
13. **Tres SO** con Git 2.38 y con la última versión estable.
14. **Sin binario y `gc`** (Enmienda 2026-10-04): sin `raptor`, `pack-refs --all` y `gc` con la rama base suelta pasan; `branch -D main` sigue saliendo con 1.
15. **Conjunto mínimo** (Enmienda 2026-10-04): sin políticas ni hooks previos solo existen `pre-push`, `reference-transaction` y `pre-rebase`; activar una política crea su dispatcher antes de aplicarla; un hook previo añadido después de instalar da `hook-previo-no-encadenado` hasta la regeneración; la regeneración interrumpida queda completa o idéntica.
16. **Gestores** (Enmienda 2026-10-04): `lefthook install` sin `--force` deja la protección activa; con `--force` o `--reset-hooks-path`, inactiva con su causa.

## Referencias

- **Reglas**: BR-AUTH-002, BR-CONS-005, BR-EDGE-002, BR-WF-002; NFR-01, NFR-07, NFR-12; S-GRD-6; Q-GRD-3, Q-GRD-4, Q-GRD-15.
- **Historias**: US-GRD-001, US-GRD-002, US-GRD-003, US-GRD-004.
- **Decisiones**: Q17 y Q22 de motor-local; decisión 4 de Rene Bonilla (2026-10-04).
- **ADRs de otros frentes** (ya en `main`): ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-013 (motor-local, en `main`); ADR-TMC-002 (time-machine, en `main`).
- **Enablers**: SPIKE-GRD-001, INF-GRD-001; INF-GRP-001 (motor-local).
- **Seguridad**: SEC-GRD-01, 02, 06, 08, 10, 12, 16, 19 en [non-functional-guardrails.md](../non-functional-guardrails.md).
- **Git**: githooks(5), git-config(1).

## Revisión de seguridad (2026-10-04)

Enmienda tras el Artifact Judge y la revisión del security-expert. No cambia la forma de la capa; endurece los dispatchers, la integridad y la actualización.

| Hallazgo | Cómo se cubre |
|---|---|
| J2 · Fail-closed en toda ref gobernada bloquea los commits | § 3: fail-closed (binario ausente y error interno) solo en `pre-push`, `pre-rebase` y el **borrado** de `refs/heads/*`; Validación 6 |
| J4 · Actualización y movimiento del binario | § 8: ruta estable con destino validado, refresco de la referencia como misma instalación con auditoría, plantilla versionada; Validación 10; casos en SPIKE-GRD-001 e INF-GRD-001 |
| J6 · Diagnóstico de encadenado imposible | § 4 paso 1 y § 6: `encadenado-imposible` guardado con el intento |
| H-04 · Ancla de integridad escribible por el agente | § 1: referencia en el diario del perfil y manifiesto no autoritativo; § 8: binario = binario del daemon, firma o huella; § 1: DACL de Windows; Validación 9 y 11 |
| M-02 · `GIT_DIR`/`GIT_WORK_TREE` cruzados | § 2: el directorio común va fijado en el dispatcher; § 3: una discrepancia es deny en refs gobernadas (SEC-GRD-19) |
| M-03 · Operaciones de archivo en `.git` | § 4: descriptor de directorio, sin seguir enlaces, borrado solo de lo listado, `dev/inode`, temporal aleatorio y exclusivo, `config` regular; Validación 8 |
| M-04 · Interpolación en el dispatcher | § 2: solo constantes con un charset seguro, encadenado en Rust sin shell, evaluación con entorno por allowlist; Validación 7 |
| M-07 · Windows | § 1: DACL; los criterios de humano y agente y del servidor del pipe están en ADR-GRD-007 § 1 y Validación 12, y ADR-GRD-003 § 4 |
| L-01 · `reference-transaction` en `committed`/`aborted` | § 2: salida en la primera línea si no hay hook previo |
| I-02 · `DYLD_INSERT_LIBRARIES` | § 2: hardened runtime sin `allow-dyld-environment-variables` |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes; los ADRs de otros frentes pasan a Referencias |
| Judge ronda 2, hallazgo 1 · Canal e id del directorio común | § 2: constantes del dispatcher; `raptor hook` no lee el diario ni el perfil; sin `HOME` en la allowlist |
| Judge ronda 2, hallazgo 2 · Fallback `sh` | § 3: código fijo con `read`, hook previo como constante, reenvío explícito de la entrada y valor relativo resuelto contra el directorio actual; Validación 7 |
| Judge ronda 2, hallazgo 3 · Daemon auténtico con perfil ajeno | § 2: id de instancia del perfil como constante; la comprobación está en ADR-GRD-003 § 4 |

## Cambios (2026-10-04, coherencia con motor-local)

- § 1: el diario de instalación vive en el almacén por repo (ADR-GRP-006 § 4).
- § 2 y § 7: dos módulos de invocación autorizados en la capa de escritura de Guardrails (encadenado del hook previo y `git` para `raptor guard exec`); `apps/cli` no lanza procesos por su cuenta.
- Las referencias a enmiendas de motor-local pasan a "aplicada (2026-10-04)".

## Enmienda (2026-10-04, SPIKE-GRD-001)

Aplicada desde las recomendaciones de [SPIKE-GRD-001-resultados.md](../../requirements/features/guardrails/research/SPIKE-GRD-001-resultados.md) (§ 9), medidas **solo en macOS**. El `status` sigue en `accepted`. Linux, Windows y el coste en Windows siguen pendientes y bloquean el merge de US-GRD-001. Cada resolución es una **decisión del orquestador (2026-10-04), validada por el Arquitecto y el PO**.

| Enmienda | Resolución | Dónde |
|---|---|---|
| E-01-1 · Código fijo del `sh` | Aceptada: la regla de borrado incluye la excepción del *prune* de `pack-refs`, leída con `read` sin lanzar programas (respaldo; la vía principal está en el binario) | § 3, Validación 14 |
| E-01-2 · Conjunto de dispatchers | Aceptada con ajustes del Arquitecto: solo obligatorios + política activa + hook previo; regeneración archivo por archivo, con lock frente a H3 y orden que cierra la ventana de las políticas; la ventana de los hooks previos añadidos después se declara y se detecta con `hook-previo-no-encadenado`; husky probablemente devuelve el conjunto completo (⚠️ ASSUMPTION) | § 2, Validación 15; ADR-GRD-005 (Enmienda) |
| E-01-3 · `reference-transaction` sin hook previo | Aceptada: sale en cualquier estado distinto de `prepared` | § 2, § 3 |
| E-01-4 · Gestores | Aceptada: fila de lefthook 2 corregida (se niega; `--force` y `--reset-hooks-path` dejan la protección inactiva) y nota de husky en los worktrees enlazados | § 6, Validación 16 |
| E-01-5 · Huella | Opción **(a)**, criterio semántico; se descarta (b). El PO ajusta BR-CONS-005 y NFR-GRD-01 (Q-GRD-29) | § 4; NFR-GRD-01 |
| E-01-6 · No se instala | Reftable **no** entra en "no se instala": se instala y el renombrado de la base se declara (ADR-GRD-002, Enmienda). Comprobación de cobertura con `--show-scope --show-origin` por worktree | § 5 |
| E-01-7 · Binario nativo | Aceptada con ajuste del Arquitecto: el dispatcher nativo es **necesario para `reference-transaction` desde Git 2.54 en los tres SO** (no solo en Windows); su forma y el respaldo sin `raptor`, en la Dev Spec de US-GRD-001 | § 2, Consecuencias |

**Casos "no se instala"** confirmados por el spike: `extensions.worktreeConfig` con `core.hooksPath` en un `config.worktree` (W03, W04, D18); `include`/`includeIf` locales o de worktree que definan la clave (W05, W07) y cualquier `includeIf "onbranch:…"` (W06). El valor no representable (M-04) sigue sin probar.

## Enmienda (2026-10-05, US-GRD-001)

Fija lo que la Enmienda de SPIKE-GRD-001 dejaba a la Dev Spec de US-GRD-001 ([DS-US-GRD-001](../../requirements/features/guardrails/dev-specs/US-GRD-001-proteger-repo-force-push.md)) y la mide en Windows (§ 14 de los [resultados del spike](../../requirements/features/guardrails/research/SPIKE-GRD-001-resultados.md)). **Decisión del orquestador (2026-10-05), validada por Arquitecto y PO.** El `status` sigue en `accepted`.

| Cambio | Resolución | Dónde |
|---|---|---|
| Forma del dispatcher nativo (§ 2) | **Nativo en los tres SO y para todos los dispatchers**, no solo `reference-transaction`: un binario mínimo `raptor-hook` (solo `std`) que se instala junto a `raptor` y se **copia tal cual** a `gitraptor/hooks/<hook>`, sin extensión. Medido en Windows: un dispatcher `sh` cuesta ≈ 43 ms por invocación y el nativo ≈ 6 ms; Git for Windows ejecuta un PE sin extensión como hook | § 2 |
| Constantes (§ 2, M-04) | En `gitraptor/dispatch.conf`, junto a los dispatchers, que el stub localiza desde su propia ruta de ejecutable: archivo regular (sin seguir enlaces), acotado, `clave<TAB>valor` estricto, hash en el diario. Meter las constantes en el binario invalidaría su firma. Sin shell, una comilla simple o `$(…)` son literales; lo no representable pasa a ser `\n`, `\r`, `\t`, NUL o bytes que no son UTF-8. El stub comprueba que la constante `common` es la ruta canónica de su propia carpeta (si no, deniega) | § 2 |
| Entorno de la evaluación (§ 2) | La allowlist añade `LC_ALL`, `LC_MESSAGES` y `LANG`: solo eligen el idioma de los mensajes (NFR-10) | § 2 |
| Código fijo sin `raptor` (§ 3) | No se genera ningún `sh`: el respaldo lo implementa el stub en Rust con las mismas reglas (incluida la excepción del *prune* de `pack-refs`) y trata también como borrado una línea de `HEAD` con valor nuevo cero. Interpreta la salida de `raptor`: 0 permite, 1 deniega y cualquier otra cosa es un error interno con la tabla del § 3. La Validación 7 (contenido del `sh`) pasa a pruebas de comportamiento del stub | § 3, Validación 7 |
| Excepción con nombre (§ 7) | El stub es el único componente de la capa de hooks con un `Command::new` propio, y solo arranca el `raptor` de sus constantes, con `env_clear` y sin shell (comprobación estática `guard_boundary`) | § 7, Validación 12 |
| Lecturas de la capa (§ 5, § 7) | `--show-scope --show-origin --get-all core.hooksPath` y los `includeIf "onbranch:"` son un perfil propio de la capa de Guardrails (`GuardRead`), no de la capa de lectura, que nunca lista configuración | § 5, § 7 |
| Alcance de US-GRD-001 (§ 4 paso 1, § 6) | Sin hooks previos que encadenar: con un hook ejecutable en `<común>/hooks` o un `core.hooksPath` en cualquier nivel, no se instala y se explica (`prior-hooks`; US-GRD-002 encadena). Así ningún hook del usuario deja de ejecutarse | § 4, § 6 |
| `manifest.json` (§ 1) | Se escribe con los pasos copiables de la recuperación manual | § 1 |

**Riesgo nuevo declarado**: tras actualizar el binario con el daemon viejo vivo, en Linux el par puede ser otro archivo y el hook deniega en refs gobernadas (`channel-not-authentic`) hasta que el daemon se reemplace. La mitigación (el daemon se cierra si cambia la identidad de su propio ejecutable) es de US-GRD-003 con el § 8.

## Enmienda (2026-10-06, US-GRD-018)

Dispatchers de la política de autoría de los commits (BR-AUTH-005; [DS-US-GRD-018](../../requirements/features/guardrails/dev-specs/US-GRD-018-autoria-commits-persona-y-agente.md), D6; ADR-GRD-003, Enmienda 2026-10-06). **Decisión del orquestador (2026-10-06), validada por el Arquitecto.** El `status` sigue en `accepted`.

| Cambio | Resolución | Dónde |
|---|---|---|
| Conjunto de dispatchers | Se añaden `pre-commit` y `commit-msg` al conjunto instalado (`pre-push`, `pre-rebase`, `reference-transaction`). El stub nativo los reconoce y su respaldo sin `raptor` es **dejar pasar con aviso** (un commit no es una operación de riesgo del mínimo) | § 1, § 2, § 3 |
| Plantilla | `TEMPLATE_VERSION = 2`; `raptor hook` acepta la 1 y la 2. Una instalación con la plantilla 1 sigue protegida por el mínimo y gana los dos dispatchers al reinstalar | § 2 |
| Hooks previos | Sin cambio: `pre-commit` y `commit-msg` ya cuentan como hooks previos que impiden instalar (US-GRD-001, D3) | § 4 |
