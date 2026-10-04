---
id: ADR-CKP-002
title: "ADR-CKP-002 — Catálogo de operaciones de usuario y ejecutor del daemon (compartido con el Servidor MCP)"
type: adr
status: proposed
created: 2026-10-04
updated: 2026-10-04
date: 2026-10-04
domain: GRP
feature: cockpit
supersedes: []
superseded_by: null
deciders: [Orquestador (delegación de Rene Bonilla, 2026-10-04)]
related:
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-008, ADR-GRP-009, ADR-GRP-012, ADR-GRP-013, ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-005, ADR-TMC-007, ADR-GRD-002, ADR-GRD-003, ADR-GRD-006, ADR-GRD-007, ADR-CKP-003]
  context: [CTX-CKP-001]
  rules: [BR-CKP-001]
  stories: []
description: "Catálogo cerrado y versionado de seis operaciones de usuario; flujo en dos fases (preparar un plan con huella, ejecutar el plan) dentro de la operación protegida; decisión de Guardrails con la capa cockpit registrada una sola vez; ejecutor en crates/core con módulo de invocación propio en crates/git, padre directo de git, sin TTY ni shell; merge sin worktree solo en avance rápido; descarte con confirmación y sin cuarentena; editor resuelto por el daemon (configuración y ruta) y lanzado por la TUI; el MCP usa el mismo catálogo"
tags: [adr, cockpit, catalogo-operaciones, ejecutor, operacion-protegida, guardrails, capa-cockpit, mcp, nfr-01, nfr-02, nfr-07, editor, descartar, merge, rebase, serializacion]
published: true
---

# ADR-CKP-002 — Catálogo de operaciones de usuario y ejecutor del daemon

**Status**: Propuesto · **Fecha**: 2026-10-04 · **Feature**: Cockpit (F-001-02), compartido con el Servidor MCP (F-001-05)

**Decisión del orquestador (2026-10-04), validada por Arquitecto.** Rene Bonilla delegó estas decisiones el 2026-10-04. El PO valida el alcance después. Las enmiendas a otros ADRs se **listan** al final y **no se aplican** aquí.

## Contexto

ADR-TMC-002 § 5 deja las operaciones de usuario al **ejecutor de operaciones del daemon**, que es de F-001-02 y F-001-05, y no de la capa de escritura de la Time Machine. ADR-TMC-004 § 1 hace de la **operación protegida** (intención, snapshot previo, ejecución, registro) la única vía de escritura de una superficie de GitRaptor. ADR-TMC-007 § 2 pide que cada operación declare si es destructiva, y ante la duda lo es. ADR-GRD-003 § 4 reconoce al ejecutor como **padre directo** del `git` y reutiliza su decisión por transición registrada. El contexto del Cockpit fija qué acciones hay (Q-CKP-8 a Q-CKP-16, Q-CKP-19), sus precondiciones (BR-CKP-ELIG-001 a 006), quién decide (BR-CKP-AUTH-001 a 003) y sus casos límite (BR-CKP-EDGE-002, 007, 008, 009). Faltan los artefactos de DEP-CKP-7, DEP-CKP-10, DEP-CKP-12 y DEP-CKP-13, y la respuesta a los riesgos R-CKP-5, R-CKP-6 y R-CKP-7.

**Constitución**: no hay `architecture-constitution.md` en la cascada. Las restricciones activas salen de AGENTS.md y de los ADRs aceptados: Rust; gitoxide para leer y Git CLI para escribir (ADR-GRP-001); monorepo con `crates/{core,policy,git,api}` (ADR-GRP-002); daemon por usuario y canal local (ADR-GRP-005); sin shell, argv fijo y allowlist (NFR-02, ADR-GRP-009); nunca push. Fuente: inline. Se formaliza con `--init-constitution`.

**Pregunta**: ¿qué operaciones puede pedir un cliente, cómo se decide y se ejecuta cada una sin otra vía de escritura, en qué entorno corre `git`, y cómo lo comparten la TUI y el MCP?

## Decisión

**Un catálogo cerrado y versionado de seis operaciones, definido en `crates/api`. Toda petición sigue dos fases: *preparar* devuelve un plan con su huella y sin escribir nada; *ejecutar* toma el cerrojo de escritura del repo, rehace el plan y solo sigue si la huella coincide. La ejecución va dentro de la operación protegida, después de la decisión de Guardrails con la capa `cockpit` (o `mcp`), que se registra una sola vez. El ejecutor vive en `crates/core` y lanza `git` desde su propio módulo de invocación de `crates/git`, como padre directo, sin TTY ni shell y respetando los hooks y la configuración del usuario.**

### 1. Catálogo (versión 1)

| Id | Qué hace | Ámbito (snapshot y permiso) | Clase (ADR-TMC-007 § 2) | Gobernada (BR-VAL-002, Guardrails) | MCP |
|---|---|---|---|---|---|
| `merge-into-base` | Integra en la base confirmada un **oid** de la rama del agente: `git merge` en el worktree que tiene la base sacada o, si ninguno la tiene, avance rápido de la ref (§ 7) | Worktree destino + ref de la base; el worktree del agente solo se lee | **Destructiva** (ante la duda: Git sobrescribe ignorados que el snapshot no guarda) | Merge | Sí (§ 12) |
| `rebase-onto-base` | Rebasa la rama del worktree del agente sobre el oid de la base confirmada | Worktree del agente + su rama | Destructiva | Rebase | Sí |
| `discard-worktree` | Quita el worktree y borra su rama; con HEAD separado, solo el worktree (§ 8) | Worktree + su rama | Destructiva | Borrar worktree + borrar rama | Sí |
| `create-worktree` | Crea rama nueva y worktree desde el oid de la base confirmada | Refs del repo | No destructiva | Crear worktree | Sí |
| `abort-in-progress` | Aborta un merge o un rebase que **dejó detenido el propio ejecutor** (§ 9) | Ese worktree | Destructiva | No (no está en BR-VAL-002) | Sí, solo sobre lo suyo |
| `open-in-editor` | Resuelve el editor configurado y valida la ruta destino; lo lanza la TUI (§ 10) | Ninguno: no escribe | Sin escritura: fuera de la operación protegida y de Guardrails (BR-CKP-ELIG-006) | No | **No** |

- **Parámetros**: tipados y validados en el daemon (SEC-02). Rutas absolutas, sin UNC, canonicalizadas y dentro de un worktree observado. Ramas con las reglas de `check-ref-format` y sin `-` inicial. Los oids van completos y salen del plan, nunca de texto libre. `create-worktree` recibe el nombre de rama nueva y una ruta opcional. Sin ruta, se usa la plantilla `cockpit.worktreePathTemplate` (perfil o local personal) o `<padre>/<repo>-<rama-saneada>` (BR-CKP-VAL-001).
- **Precondiciones**: las de la matriz BR-CKP-ELIG-001, **todas revalidadas en el daemon** al preparar y otra vez al ejecutar, con el estado que el motor publica y lecturas de `gix`. El ejecutor añade cuatro de Git que la TUI no ve: sin locks de Git en el ámbito (`index.lock`, `HEAD.lock`, locks de refs; se informa "Git ocupado" y **nunca se borran**, como ADR-TMC-002 § 3.1); worktree no bloqueado con `git worktree lock`; rama del agente no sacada en otro worktree; en `create-worktree`, ruta inexistente y fuera de cualquier worktree.
- **Ver diff** no es una operación del catálogo: es una consulta del canal (DEP-CKP-3).
- **Versión**: `catalogVersion = 1`, negociada en el handshake del canal. Añadir una operación o un parámetro opcional sube la versión menor. Cambiar una semántica, la clase o la gobernanza sube la mayor y exige revisar este ADR. Una operación desconocida se rechaza (`deny_unknown_fields`). El canal expone una consulta `describe` con los ids, el esquema de parámetros, la clase y la marca MCP. Los textos los pone cada cliente (i18n en/es, NFR-10).

### 2. Flujo: preparar y ejecutar

```mermaid
sequenceDiagram
  participant C as TUI / raptor-mcp
  participant D as Daemon (executor)
  participant P as crates/policy
  participant T as Time Machine
  participant G as git (hijo directo)
  C->>D: prepare(op, params, secuencia vista)
  D->>D: solicitante por ascendencia, precondiciones, plan + huella
  D->>P: evaluar(op normalizada, capa cockpit|mcp, actor)
  D-->>C: plan, avisos, decisión, confirmaciones requeridas, reto
  C->>D: execute(huella, avisos aceptados, respuesta al reto)
  D->>D: cerrojo de escritura del repo; rehacer plan; comparar huella
  D->>T: intención (oplog)
  D->>P: re-evaluar (la que cuenta)
  D->>T: snapshot previo garantizado (si falla, abortada)
  D->>G: spawn + registro de la identidad del hijo y sus transiciones
  G-->>D: código de salida; el daemon lee el estado resultante con gix
  D->>T: registro del resultado
  D-->>C: resultado + evento en el stream de todos los clientes
```

1. **Preparar** (sin efectos y sin oplog): el daemon resuelve el solicitante (§ 3), comprueba las precondiciones y construye el **plan**: la operación, los parámetros, los valores esperados (oid de la base, punta de la rama, HEAD de cada worktree del ámbito), la marca de secuencia del motor, la huella del working tree que publica el motor, los avisos (⚡, sesión Activa con "se integra hasta el commit X") y, en `discard-worktree`, la lista de lo no recuperable (§ 8). La **huella** es el hash del plan. El daemon evalúa Guardrails como vista previa y la devuelve.
2. **Confirmaciones**: los avisos son UX. La petición de ejecutar debe enumerar los códigos de aviso del plan; si no coinciden, se rechaza. Las confirmaciones que **son controles** (trabajo de otro actor, § 3; excepción consciente, § 4) las verifica el daemon.
3. **Ejecutar**: el daemon toma el cerrojo (§ 5), anota la **intención** en el oplog (ADR-TMC-003 § 3) y rehace el plan. Si la huella cambió: `rechazada` con "el estado cambió", sin efectos (BR-CKP-CONS-004). Después re-evalúa Guardrails; **esa decisión es la que cuenta**. Se toma antes de cualquier efecto en el repo, snapshot incluido (BR-CKP-WF-002). Sigue el snapshot previo garantizado; si falla, `abortada` sin cambios (D-TMC-10). Después la ejecución (§ 6) y el registro. La intención es un apunte del oplog, no un efecto en el repo.
4. **Resultado**: `done`, `stopped` (Git dejó una operación en curso, § 9), `failed-unchanged` (Git falló y el daemon comprueba que el ámbito es igual al previo), `failed-changed` (falló con cambios: se ofrece Deshacer), `rejected`, `aborted` (snapshot) o `cancelled`. Cada resultado lleva el código, el motivo tipado y la salida de Git. Esa salida es texto no confiable, se trunca y solo va al cliente que lo pidió, nunca al perfil (SEC-05) ni al MCP (§ 12). El inicio y el fin de cada operación se publican en el stream para todos los clientes (Q-CKP-19).
5. **Caducidad del plan**: ⚠️ **ASSUMPTION**: 60 s, igual que el reto de ADR-TMC-005 § 3. Un plan caducado obliga a preparar otra vez.

### 3. Solicitante, permisos y confirmación ligada al plan (Q-CKP-16)

- **Solicitante**: lo resuelve el daemon por ascendencia, con el identificador no reutilizable del canal (ADR-TMC-005 § 1). El plan lo devuelve y la TUI lo muestra ("Actúas como claude-1", BR-CKP-AUTH-002; R-CKP-7).
- **Trabajo afectado** por operación: `merge-into-base`, los commits que entran en la base; `rebase-onto-base`, los commits reescritos; `discard-worktree`, los commits que no están en la base más los cambios sin commitear (si no hay nada, no afecta a nadie); `abort-in-progress`, lo que dejó la operación detenida. El actor de cada parte sale de los eventos del motor (ADR-GRP-013). **Sin atribución clara o con atribución mixta, cuenta como otro actor** (conservador, como ADR-TMC-005 § 5).
- **Regla base**: la tabla de ADR-TMC-005 § 2, extendida al catálogo. Un agente X solo actúa sobre trabajo de X; sobre otro, rechazo. Un "sin atribuir" por CLI/TUI actúa sobre trabajo sin atribuir sin control; sobre trabajo de un agente, necesita la **confirmación interactiva ligada al plan**. Un "sin atribuir" por MCP, rechazo (TQ-7).
- **Confirmación ligada al plan**: reutiliza ADR-TMC-005 § 3 sin duplicarlo. El daemon comprueba los controles 1 a 3 de ADR-GRP-005 § 6 y emite un **reto de un solo uso** ligado a la conexión, al proceso y a la **huella del plan**. Se invalida si el plan cambia. El ConfirmPrompt de la TUI muestra ese plan; es UX. Un mismo prompt reúne todos los motivos (⚡, sesión Activa, trabajo ajeno).
- **Windows**: sin confirmación de trabajo ajeno; la operación se rechaza con su motivo (TQ-14, BR-CKP-AUTH-003). Pendiente: etapa de validación multiplataforma.

### 4. Decisión de Guardrails: capa `cockpit`, registrada una sola vez (DEP-CKP-10)

- **Evaluación**: la misma función pura de ADR-GRD-003 § 1, con la operación normalizada del plan (transiciones exactas cuando se conocen), el actor resuelto y la capa `cockpit` (TUI) o `mcp`. "Pedir confirmación" se aplica como denegar mientras no haya cola (S-GRD-9). Es la única protección para merge y borrar worktree, que los hooks no impiden (ADR-GRD-002).
- **Ligadura con los hooks**: al lanzar `git`, el ejecutor registra **en el mismo paso** la identidad del hijo (pidfd, audit token o handle con hora de inicio) y las transiciones del plan. Las que no se conocen de antemano se registran como en ADR-GRD-007 § 3.4: ref más base en el rebase; ref, valor viejo y oid integrado en el merge. Cuando un hook de Guardrails pregunta y el `git` más cercano es ese hijo, con el daemon como padre directo, recibe la decisión ya tomada. Otra transición se evalúa de nuevo (ADR-GRD-003 § 4 y § 6). Con ese hijo no se pide el snapshot `previo_hook`, porque ya existe el `previo_garantizado`. **La evaluación de un hook nunca espera al cerrojo de escritura del repo**; si lo hiciera, el hook de un `git` del ejecutor se bloquearía con él.
- **Registro único**: cada plan deja **como mucho una** entrada en el registro de ADR-GRD-006, con `layer = cockpit` (o `mcp`), escrita **al cerrarse el plan**. El `kind` depende del desenlace: `denial` si se denegó y el humano no siguió o el plan caducó; `exception`, `exception-rejected` o `exception-cancelled` si siguió por la excepción; ninguna si se permitió sin regla. Las evaluaciones de hooks bajo el ejecutor no crean entradas.
- **Excepción consciente desde el Cockpit** (Q-CKP-15, ADR-GRD-007 § 1, D10): es un comando reservado sobre el proceso de la TUI (controles 1 a 3), con anuncio `reserved-action-pending`, ventana cancelable (⚠️ **ASSUMPTION** heredada: 10 s, S-CKP-3) y auditoría con la ascendencia completa y la aceptación del riesgo. **No emite un token en el entorno**: el emisor y el padre del `git` son el mismo daemon. La ligadura de un solo uso a la transición exacta la da el registro del hijo y la huella del plan. Se rechaza si el solicitante es un agente y nunca se ofrece por MCP.

### 5. Serialización y revalidación (Q-CKP-19)

- **Un cerrojo de escritura por repo** (clave del directorio común) en el daemon, **compartido con el aplicador de la Time Machine** (undo, redo, restauración; ADR-TMC-002 § 3.3). Vive en un módulo neutro de `crates/core` que los dos usan. Así un merge y un undo nunca se mezclan. No se usa un cerrojo por worktree, porque un merge toca dos worktrees y refs del repo.
- **Cola**: las peticiones del mismo repo esperan en orden de llegada y se ven como "en cola" en todos los clientes. Al obtener el cerrojo, la huella se compara de nuevo. Dos TUIs que descartan el mismo worktree: la primera se ejecuta y la segunda recibe "el worktree ya no existe" (BR-CKP-CONS-004). Los topes de la cola los fija la Dev Spec.
- **Contra procesos externos**, que el cerrojo no ve: los valores esperados del plan se pasan a Git donde Git lo admite (merge de un oid y no de un nombre de rama; actualizar y borrar refs con el valor viejo). Si no se puede, se comprueba bajo el cerrojo justo antes de lanzar `git`. El riesgo residual es la ventana entre esa comprobación y Git, y lo cubre el snapshot previo.

### 6. Entorno del ejecutor (R-CKP-5)

- **Padre directo**: el daemon lanza `git` directamente, sin shell ni procesos intermedios. Usa el binario que ya resolvió (ADR-GRP-009 § 4; nota E6 de ADR-TMC-002), con argv fijo por operación y `--` antes de las rutas donde Git lo admite. El cwd es la raíz del worktree tomada del estado validado del daemon. Se lanza en una **sesión nueva sin terminal de control** (`setsid`), con stdin a nulo y stdout y stderr capturados con tope. Windows: `CREATE_NO_WINDOW`, sin consola. Pendiente: etapa de validación multiplataforma.
- **Se respeta lo del usuario** (NFR-07, ADR-TMC-002 § 5): los hooks (incluido `core.hooksPath` y el despachador de Guardrails), los filtros `clean`/`smudge`/LFS, los drivers de merge, `rerere`, la identidad, la firma (`commit.gpgsign`, `gpg.format`) y `merge.ff`. Una `merge.ff=only` que impide el merge da `failed-unchanged`, con el motivo y la acción.
- **Se neutraliza**, con `-c` y variables fijas:
  - Los ejecutables de ADR-GRD-007 § 3.5: `core.fsmonitor`, `core.pager`, `core.editor`, `sequence.editor`, diff externo y `textconv`.
  - **Editor**: `GIT_EDITOR` y `GIT_SEQUENCE_EDITOR` apuntan a un **ejecutable de rechazo** fijo, con ruta absoluta y sin metacaracteres, así que Git lo ejecuta sin shell. Es el binario `raptor` instalado con un subcomando interno que termina con error. Además, `GIT_MERGE_AUTOEDIT=no`, merge con `--no-edit` y un mensaje con plantilla fija que solo lleva el nombre de la rama validado.
  - `GIT_TERMINAL_PROMPT=0`, sin `GIT_ASKPASS` ni `SSH_ASKPASS`; ninguna operación del catálogo usa la red y **nunca hay push ni fetch** (BR-CKP-CONS-002).
  - `gc.auto=0` y `maintenance.auto=false`: el `gc` desacoplado escaparía del árbol de procesos del ejecutor. El usuario lo tendrá en su siguiente `git`.
  - `rebase.updateRefs=false` y `--no-autosquash`: el rebase no mueve otras ramas, que pueden ser de otros actores.
- **Variables de entorno**: el entorno se construye desde cero con la allowlist de ADR-GRP-009 § 3 (sin `GIT_*`, `LD_*`, `DYLD_*`, `GIT_SSH_COMMAND`, `XDG_CONFIG_HOME`), más las fijas de arriba, más una **lista cerrada de variables de sesión** que el cliente declara en la petición y el daemon valida: `PATH` (solo entradas absolutas), `SSH_AUTH_SOCK`, `GNUPGHOME`, `LANG` y `LC_*`. Sin ellas, los hooks del usuario (p. ej. los que llaman a `node`) y la firma SSH fallarían con el PATH mínimo del daemon. ⚠️ **ASSUMPTION**: con esta lista basta para los hooks y la firma habituales; lo verifica la Validación 9.
- **Si un hook o la firma piden interacción**: no hay TTY, así que `/dev/tty` falla y stdin da EOF. Un editor pedido acaba en el ejecutable de rechazo. El resultado es un fallo de Git, nunca una espera en una terminal invisible. Si Git deja la operación a medias (un `commit-msg` que rechaza el commit de merge, un `gpg` que no firma), el resultado es `stopped` y aplica el § 9. Si Git falla sin cambios, `failed-unchanged`. La TUI muestra la salida de Git saneada (SEC-12).
- **Sin límite de tiempo automático**: matar `git` a mitad deja locks y estados a medias. La TUI muestra el tiempo transcurrido y ofrece **Cancelar**, que envía una interrupción al grupo de procesos, como Ctrl-C. Lo puede pedir la conexión solicitante o, si se cerró, cualquier cliente CLI/TUI del usuario; nunca el MCP. Lo que Git deje se trata como `stopped` o `failed-changed`.
- **Caída del daemon** a mitad: la operación queda `interrumpida` (ADR-TMC-003 § 6) y Deshacer vuelve al previo. El `git` huérfano puede terminar, y sus hooks caen en el modo degradado de Guardrails, que es más estricto. En Linux el hijo muere con el daemon (señal de muerte del padre); en macOS y Windows, no. Pendiente: etapa de validación multiplataforma.

### 7. Merge cuando ningún worktree tiene la base sacada (BR-CKP-EDGE-009)

- **Con un worktree que tiene la base sacada** (es único): `git merge` del oid del plan en ese worktree, con las precondiciones de BR-CKP-ELIG-002.
- **Sin ninguno**: solo **avance rápido**. Si el oid desciende de la punta esperada de la base, el ejecutor actualiza `refs/heads/<base>` con el valor viejo esperado, en una sola transacción de ref. Antes comprueba, bajo el cerrojo, que ningún worktree tiene la base sacada. No hay working tree que actualizar. El hook `reference-transaction` sí corre, y Guardrails ve la transición.
- **Sin ninguno y sin avance rápido**: rechazo con el motivo y la acción "saca la base en un worktree para integrar".
- Respeta "nunca push" y "nunca escribir fuera de la operación protegida": no crea worktrees temporales ni escribe objetos fuera de Git.

### 8. Descartar: confirmación que nombra lo no recuperable, sin cuarentena (R-CKP-6)

- **Ejecución**: `git worktree remove --force` y después el borrado de la rama con el valor viejo esperado. Con HEAD separado, solo el worktree. Un worktree bloqueado se rechaza; no se usa el doble `--force`. La sección `branch.<rama>.*` de la config se conserva, y así Deshacer devuelve la rama con su upstream.
- **Qué no recupera el snapshot** (ADR-TMC-001 § 2), calculado **en el daemon** al preparar y parte de la huella: ignorados, la lista cerrada de credenciales, el contenido de los submódulos y los repos anidados. **Los archivos grandes sí se recuperan**: el previo garantizado los incluye siempre. La lista va resumida (directorios colapsados, recuento y tamaño) y acotada en tiempo. **Si el recorrido no termina, el resultado nunca dice "nada"**: dice "puede haber más contenido no recuperable" y exige confirmación.
- **Confirmaciones**: las de BR-CKP-ELIG-004 y BR-CKP-EDGE-008. Con algo no recuperable, el ConfirmPrompt es obligatorio (default No) y lo nombra. El toast dice "recupera todo salvo lo listado".
- **Cuarentena: descartada en el MVP**. Se revisa tras el dogfooding si el registro muestra descartes confirmados con pérdida (ver Opciones).

### 9. Operación detenida por conflicto (Q-CKP-11)

- Un merge o un rebase que choca se queda **como lo deja Git**. Es `stopped`, con las rutas sin fusionar (el motor las publica: DEP-CKP-14). El ejecutor no aborta solo ni resuelve nada.
- **Abortar** es `abort-in-progress`, una operación protegida. Solo se ofrece si el oplog registra que **el ejecutor** dejó esa operación detenida en ese worktree y el estado coincide (BR-CKP-EDGE-002). Su snapshot previo guarda las resoluciones a medias como archivos.
- **Abrir en el editor** se ofrece siempre (§ 10).
- **Deshacer** solo después de abortar: la Time Machine rechaza con una operación en curso (BR-TMC-EDGE-004). El estado en conflicto no se restaura tal cual: los pseudo-refs como `MERGE_HEAD` no están en el snapshot.

### 10. Abrir en el editor (DEP-CKP-12, DEP-CKP-13)

- **El daemon resuelve y valida, la TUI lanza** (coherente con ADR-CKP-003 § 9). `open-in-editor` en el daemon devuelve el argv de `cockpit.editor` (perfil o local personal), si existe, y valida que la ruta destino está dentro de un worktree observado (SEC-02). Sin esa clave, la TUI usa su `$VISUAL` y después su `$EDITOR`. En los dos casos, la separación en palabras sin shell y el rechazo de metacaracteres (BR-CKP-VAL-003) son **una sola función pura de `crates/api`**, compartida por el daemon y la TUI. Sin editor: error accionable (BR-CKP-EDGE-007).
- **Tipo de editor**: lo clasifica la TUI con una lista conocida; lo desconocido se trata como de terminal (ADR-CKP-003 § 9). La clave opcional `cockpit.editorKind` (`auto`, `terminal`, `gui`) fuerza la clasificación. Con uno de terminal, la TUI suspende la pantalla, lanza el editor con su terminal y espera. Con uno gráfico, lo lanza desacoplado y no espera.
- **Por qué lanza la TUI**: un editor de terminal necesita la terminal de la TUI, y uno gráfico debe heredar el entorno de la sesión del usuario, no el entorno limpio del daemon. Así el daemon solo lanza `git`.
- **Dónde**: el módulo de lanzamiento autorizado y nombrado `tui::editor` de `apps/cli` (ADR-CKP-003 § 9). Resuelve `argv[0]` a ruta absoluta con un PATH sin entradas relativas y sin el cwd, y solo ejecuta un argv que ya pasó la validación. Es el único `Command::new` de la TUI.
- **Clave del editor**: solo en el perfil y en la local personal; en la del equipo se ignora con diagnóstico (Q24 de motor-local, DEP-CKP-13). El MCP nunca abre el editor.

### 11. Código y fronteras

- **`crates/api`**: el catálogo (ids, parámetros, clase, marca MCP, `catalogVersion`), el plan, la huella, los avisos, los resultados y los códigos de motivo.
- **`crates/core`, módulo `executor`**: preparar, ejecutar, la cola y el registro de hijos para Guardrails. Usa la API pública de la operación protegida del módulo `timemachine` (intención, previo, registro). Lanzar requiere un **tipo que solo construye la Time Machine al completar el previo**, así que "sin snapshot no hay operación" se cumple por construcción. Lee con la capa de lectura de ADR-GRP-009.
- **`crates/git`, módulo de invocación de operaciones de usuario**: una lista cerrada y tipada (merge de un oid, abortar merge, rebase sobre un oid, abortar rebase, añadir worktree con rama nueva, quitar worktree, actualizar ref con valor viejo, borrar ref con valor viejo), con el entorno del § 6. Es el único sitio donde el ejecutor lanza procesos.
- **Prohibido**, con una comprobación estática en CI: que `executor` importe la capa de escritura de la Time Machine (ADR-TMC-002 § 1), las capas de escritura de Guardrails o sus módulos de invocación (ADR-GRD-001 § 7), y que alguien que no sea `executor` importe el módulo de operaciones de usuario.
- **`crates/policy`**: sin cambios de forma; solo admite la capa `cockpit`.

### 12. Uso desde el Servidor MCP (F-001-05)

- **El mismo catálogo y el mismo flujo**: preparar y ejecutar, las mismas validaciones (SEC-02), argv fijo, sin shell (NFR-02) y allowlist de repos. El repo y el worktree salen del cwd del llamante (ADR-TMC-005 § 1). Capa `mcp` (ADR-GRD-003 § 5).
- **Avisos**: el agente debe enumerar los códigos de aviso del plan al ejecutar, igual que la TUI.
- **Controles**: lo que necesita confirmación de trabajo ajeno o excepción **se rechaza** por MCP. Un "sin atribuir" por MCP se rechaza siempre.
- **Respuestas**: estructuradas, solo con códigos y campos tipados. Sin la salida de Git ni de los hooks, sin mensajes de commit ni contenido, y limitadas al repo del llamante (SEC-12, OWASP LLM01).
- **Lo que fija F-001-05**: qué operaciones con marca MCP publica como herramienta. **No puede ampliar el catálogo**: `open-in-editor` y los comandos reservados quedan fuera.

## Opciones consideradas

| Tema | Opción | En contra | Veredicto |
|---|---|---|---|
| Dónde vive | **`executor` en `crates/core` + módulo de invocación en `crates/git`** | Un módulo más en la comprobación estática | **Elegida**: sigue el patrón de la Time Machine y Guardrails y no enmienda ADR-GRP-002 |
| | Crate nuevo `crates/executor` | Enmienda ADR-GRP-002 y duplica la invocación de Git | Descartada |
| | Dentro de la capa de escritura de la Time Machine | Sin hooks del usuario; lo prohíbe ADR-TMC-002 | Descartada |
| | Ejecutar en el cliente | Otra vía de escritura; solicitante en un cliente no confiable (ADR-TMC-004) | Descartada |
| Flujo | **Preparar y ejecutar con huella** | Una ida y vuelta más | **Elegida**: confirmaciones ligadas al plan y revalidación con varias TUIs |
| | Una sola llamada | La confirmación no se puede ligar a lo que se mostró | Descartada |
| Merge sin la base sacada | **Solo avance rápido de la ref con valor viejo** | El merge con commit pide sacar la base | **Elegida**: sin escritura fuera de Git ni hooks saltados |
| | Worktree temporal del ejecutor | Escribe fuera del repo y del perfil, ejecuta hooks de checkout en una ruta que el usuario no conoce y deja residuos si se cae | Descartada |
| | `merge-tree` + `commit-tree` + actualizar la ref | Salta `pre-merge-commit` y `commit-msg` (rompe NFR-07) y un conflicto no tiene salida | Descartada |
| | Exigir siempre un worktree | Rechaza un avance rápido inocuo | Descartada |
| Descartar | **Confirmación que nombra lo no recuperable** | Lo ignorado se pierde si el humano confirma | **Elegida**: ya aceptada por BR-CKP-EDGE-008 |
| | Cuarentena en el perfil | Copia credenciales al perfil (contra SEC-TMC-06), `node_modules` agota las cuotas y añade otro ciclo de retención y purga | Descartada en el MVP |
| | Apartar el directorio en el mismo volumen, o papelera del SO | Escribe fuera del repo y del perfil (Q17); entre volúmenes es una copia; la API depende del SO | Descartada en el MVP; candidata tras el dogfooding |
| Editor | **El daemon resuelve la configuración y valida la ruta, la TUI lanza** | Un módulo de lanzamiento autorizado en `apps/cli` | **Elegida** (coherente con ADR-CKP-003 § 9) |
| | Lo lanza el daemon | Sin la terminal de la TUI (editores de terminal) y con el entorno limpio | Descartada |
| | Lo resuelve la TUI sola | No puede leer el perfil ni la configuración (ADR-GRP-005) | Descartada |
| Interacción | **Sin TTY + editor de rechazo** | Un hook interactivo falla | **Elegida**: falla y se ve; nunca se queda colgado |
| | Pty propia del daemon | El prompt quedaría en una terminal invisible | Descartada |
| | `git` en la terminal de la TUI | La TUI sería el padre de `git` y escribiría el cliente | Descartada |
| Entorno | **Allowlist + variables de sesión declaradas** | El cliente declara su PATH | **Elegida**: un agente ya puede lanzar `git` con su PATH; no gana nada |
| | Entorno limpio del daemon | Los hooks con `node` y la firma SSH fallan | Descartada |
| | Entorno completo del cliente | `LD_PRELOAD`, `GIT_*` (SEC-10) | Descartada |
| Serialización | **Cerrojo por repo compartido con la Time Machine** | Un hook lento retiene el repo | **Elegida** |
| | Por worktree | Un merge toca dos worktrees y refs del repo | Descartada |

## Consecuencias

- ✅ Una sola vía de escritura (ADR-TMC-004): cada operación del catálogo pasa por la operación protegida, y el tipo del previo lo garantiza en compilación.
- ✅ La TUI y el MCP comparten catálogo, validaciones y decisión. Solo cambia la capa y lo que se puede confirmar.
- ✅ Guardrails gobierna merge y borrar worktree desde el Cockpit, aunque los hooks no los impidan, con una entrada por operación.
- ✅ Varias TUIs no se pisan: cerrojo, cola y huella.
- ⚠️ **Un hook lento** (p. ej. tests en `pre-merge-commit`) retiene el cerrojo del repo y el resto de operaciones esperan. **Mitigación**: el estado "en cola" se ve y existe Cancelar.
- ⚠️ **Los hooks interactivos fallan** desde el Cockpit (R-CKP-5). **Mitigación**: el fallo se ve con la salida de Git y la operación se recupera; el usuario puede repetirla desde su terminal.
- ⚠️ **Desviaciones declaradas de NFR-07**: `gc.auto` y `maintenance.auto` desactivados, `rebase.updateRefs` y el autosquash desactivados, editores rechazados, y sin `post-merge` en el avance rápido sin worktree.
- ⚠️ **En Windows**, el "sin atribuir" no puede integrar ni descartar trabajo de un agente desde el Cockpit (TQ-14). Pendiente: etapa de validación multiplataforma.
- ⚠️ **Descartar puede perder lo ignorado**, nombrado antes de confirmar. **Mitigación**: confirmación obligatoria y revisión tras el dogfooding.
- ⚠️ **La lista de variables de sesión** puede quedarse corta para algún hook. **Mitigación**: el fallo se ve; ampliar la lista exige revisar este ADR.

## Validación

Repos y perfiles temporales; nunca este repo.

1. **Una vía**: test de contrato de `crates/api`: ninguna operación del catálogo escribe sin una fila `previo_garantizado`; con el almacén lleno, `merge-into-base` da `aborted` y el repo no cambia.
2. **Frontera**: la comprobación estática falla si `executor` importa la capa de escritura de la Time Machine, las de Guardrails o sus módulos de invocación, o si otro módulo importa la invocación de operaciones de usuario. Auditoría dinámica de `exec` (INF-GRP-001): todo hijo `git` del ejecutor tiene el daemon como padre directo.
3. **Huella**: dos TUIs descartan el mismo worktree; la segunda recibe "el worktree ya no existe". Un agente simulado commitea entre preparar y ejecutar un rebase: `rejected`, sin cambios.
4. **Valor esperado**: con la base movida por un proceso externo entre la comprobación y Git, la actualización de la ref falla entera.
5. **Registro único**: un merge permitido sin regla no deja entrada. Un borrado de rama denegado deja una `denial` con `layer = cockpit`. Una excepción aplicada deja una sola `exception`, aunque los hooks pregunten varias veces.
6. **Ligadura**: un `git` lanzado por un hook del usuario durante la operación (nieto del daemon) no hereda la decisión. Una transición distinta de la registrada se evalúa de nuevo.
7. **Sin interbloqueo**: un hook de Guardrails bajo el ejecutor recibe su decisión mientras el cerrojo del repo está tomado.
8. **Entorno sin TTY**: con un `prepare-commit-msg` que lee `/dev/tty`, `core.editor` y `sequence.editor` apuntando a un canario y un `commit-msg` que rechaza: el canario no se ejecuta, nada se queda colgado y el resultado es `stopped` o `failed-unchanged` con la salida saneada.
9. **Lo del usuario**: un hook con `node` del PATH de la sesión y un merge firmado con SSH funcionan. Con `rebase.updateRefs=true` en la config, otras ramas no se mueven.
10. **Merge sin la base sacada**: con avance rápido, la base avanza, no se crea ningún worktree y `reference-transaction` corre. Sin avance rápido, rechazo con la acción.
11. **Descartar**: con `.env.local`, `node_modules/` y un submódulo, la confirmación los nombra. Un archivo de 200 MB sin seguimiento se recupera con Deshacer. Un worktree bloqueado se rechaza.
12. **Detenido**: un merge que choca queda `stopped` con sus rutas. Abortar solo se ofrece si lo detuvo el ejecutor. Tras abortar, Deshacer está disponible.
13. **Trabajo ajeno**: "sin atribuir" en macOS confirma el descarte de un worktree de claude-2 con el reto ligado. El reto reutilizado o el de un plan cambiado se rechazan. claude-1 sobre trabajo de claude-2: rechazo. En Windows: rechazo con motivo.
14. **Editor**: `code --wait`, `vim` y `vim; rm -rf ~` (este último rechazado). `cockpit.editor` en la configuración del equipo, ignorado con diagnóstico. El MCP no ofrece `open-in-editor`.
15. **MCP**: un "sin atribuir" se rechaza; un agente que no enumera los avisos se rechaza; las respuestas no llevan la salida de Git (instantánea de la respuesta).
16. **Nunca push**: captura de red durante todo el catálogo: 0 conexiones.

## Enmiendas que implica (no aplicadas)

**Estado (2026-10-04)**: todas las filas aplicadas como "Enmienda (2026-10-04, Cockpit)" en el ADR de destino. El contrato del canal sigue pendiente (ver Pendientes).

| ADR | Sección | Texto propuesto (breve) | Origen |
|---|---|---|---|
| ADR-GRP-009 | Validación 5 y nota de integración | Añadir a los módulos autorizados (1) la **invocación de operaciones de usuario** de `crates/git`, que solo importa `crates/core::executor`, y (2) el **lanzador del editor** `tui::editor` de `apps/cli` (ADR-CKP-003 § 9), que solo ejecuta un argv validado por la función pura de `crates/api`. `executor` no importa ninguna capa de escritura ni los módulos de invocación de Guardrails. El autoarranque (overview § 10.5) lo cierra la enmienda E3 de ADR-CKP-003: módulo de arranque de la biblioteca cliente de `crates/api` (ajuste de coherencia, 2026-10-04) | DEP-CKP-7, DEP-CKP-12 |
| ADR-GRP-009 | § 4 | El ejecutor usa el mismo binario de Git resuelto (une la nota E6 de ADR-TMC-002) | DEP-CKP-7 |
| ADR-GRD-003 | § 1 | Capa del contexto: `hooks` \| `mcp` \| `cockpit` | DEP-CKP-10 |
| ADR-GRD-003 | § 4 y § 5 | El ejecutor registra el hijo y sus transiciones con la capa de la petición. Con un `git` del ejecutor no se pide `previo_hook`. La evaluación del hook no espera al cerrojo de escritura del repo. La capa `cockpit` sigue el contrato del § 5: decidir antes de la operación protegida | DEP-CKP-10 |
| ADR-GRD-006 | § 1 y § 6 | `layer` admite `cockpit`. Una operación del ejecutor deja como mucho una entrada, escrita al cerrarse el plan, con el `kind` del desenlace. Las evaluaciones de hooks bajo el ejecutor no crean entradas | DEP-CKP-10 |
| ADR-GRD-007 | § 1, "Interfaz para el Cockpit" | La excepción del Cockpit no emite token en el entorno: la ligadura es el registro del hijo del ejecutor más la huella del plan, con los mismos controles, D5 y D10 | Q-CKP-15 |
| ADR-TMC-005 | § 2 y § 3 | La regla base y la confirmación ligada al plan se extienden a las operaciones del catálogo, con la definición de "trabajo afectado" del § 3 de este ADR; Windows rechaza (TQ-14) | Q-CKP-16, BR-CKP-AUTH-003 |
| ADR-TMC-002 | § 3.3 y § 5 | El cerrojo por repo del aplicador es el mismo cerrojo de escritura del repo que usa el ejecutor; el catálogo es el de ADR-CKP-002 | Q-CKP-19 |
| ADR-GRP-007 | Tabla de claves | `cockpit.editor` (string; perfil y local; equipo no admitido), `cockpit.editorKind` (`auto`\|`terminal`\|`gui`; perfil y local) y `cockpit.worktreePathTemplate` (string; perfil y local) | DEP-CKP-13, BR-CKP-VAL-001 |
| ADR-GRP-008 | Extracto de niveles | El nivel local personal admite esas tres claves | DEP-CKP-13 |

## Pendientes

- **Contrato del canal** (pendiente, dueño: worker del canal, TS-GRP-004): los métodos `describe`, `prepare`, `execute` y `cancel`; `catalogVersion` en el handshake; los eventos de inicio, fin y cola de operación; la declaración de variables de sesión en la petición. No se aplica aquí (DEP-CKP-6).
- **Motor-local**: publicar el estado en conflicto con sus rutas (DEP-CKP-14) y la atribución commit→evento para "trabajo afectado" (DEP-CKP-2, opcional). Sin ella, cuenta como otro actor.
- **Para el PO**: en BR-CKP-EDGE-008, "excluidos por tamaño" sobra (el previo garantizado los incluye) y faltan los repos anidados. En BR-CKP-AUTH-003 falta el rebase como trabajo ajeno. En BR-CKP-ELIG-004 conviene añadir el worktree bloqueado.
- **Pendiente: etapa de validación multiplataforma**: sesión sin terminal y consola en Windows, muerte del hijo con el daemon en macOS y Windows, confirmación de trabajo ajeno en Windows y la lista de variables de sesión en Linux y Windows.
- **Revisión de seguridad**: falta la del security-expert sobre el entorno del § 6 y la excepción del § 4 antes de `accepted`.

## Referencias

- **Contexto y reglas**: CTX-CKP-001 (Q-CKP-8 a Q-CKP-16, Q-CKP-19; DEP-CKP-7, 10, 12, 13; R-CKP-5, 6, 7); BR-CKP-ELIG-001 a 006, BR-CKP-AUTH-001 a 003, BR-CKP-WF-002, BR-CKP-WF-003, BR-CKP-CONS-002, BR-CKP-CONS-004, BR-CKP-VAL-001, BR-CKP-VAL-003, BR-CKP-EDGE-002, 007, 008, 009.
- **ADRs**: ADR-GRP-005 § 5 y § 6, ADR-GRP-009 § 3 y § 4, ADR-GRP-013; ADR-TMC-001 § 2, ADR-TMC-002 § 3 y § 5, ADR-TMC-003 § 3 y § 6, ADR-TMC-004 § 1, ADR-TMC-005, ADR-TMC-007 § 2; ADR-GRD-002, ADR-GRD-003 § 1, § 4, § 5 y § 6, ADR-GRD-006, ADR-GRD-007 § 1 y § 3.
- **NFR y seguridad**: NFR-01, NFR-02, NFR-03, NFR-07, NFR-10; SEC-02, SEC-05, SEC-10, SEC-12; SEC-TMC-03, SEC-TMC-06.
