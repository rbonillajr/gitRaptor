---
id: BR-TMC-001
title: "Reglas de Negocio — Time Machine"
type: business-rules
status: draft
created: 2026-10-03
updated: 2026-10-28
domain: GRP
epic: E-001
feature: time-machine
related:
  context:
    - CTX-TMC-001
  stories:
    - US-TMC-001
    - US-TMC-002
    - US-TMC-003
    - US-TMC-004
    - US-TMC-005
    - US-TMC-006
    - US-TMC-007
    - US-TMC-008
    - US-TMC-009
    - US-TMC-010
    - US-TMC-011
    - US-TMC-012
    - US-TMC-013
    - US-TMC-014
    - US-TMC-015
    - US-TMC-016
    - US-TMC-017
    - US-TMC-018
    - US-TMC-019
    - US-TMC-020
    - US-TMC-021
    - US-TMC-022
    - US-TMC-024
tags:
  - time-machine
  - snapshots
  - undo
  - timeline
---

# Reglas de Negocio: Time Machine

> **Propósito**: reglas que gobiernan los snapshots, el undo/redo, la restauración y el timeline. Los códigos llevan el prefijo `TMC` para no colisionar con los del Motor local (BR-CONS-001, BR-EDGE-004, etc.).

---

## Contexto

**Feature**: Time Machine (F-001-03)
**Enlace a contexto**: [context.md](./context.md) (CTX-TMC-001)
**Última actualización**: 2026-10-27

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos | 1 | 0 |
| Workflows y Estados | 3 | 3 |
| Permisos y Autorizaciones | 1 | 1 |
| Consistencia de Datos | 5 | 5 |
| Tiempo y Expiración | 1 | 0 |
| Edge Cases | 4 | 3 |

> Cálculos de Negocio y Elegibilidad: no aplican a esta feature.

---

## 1. Validaciones de Datos

### BR-TMC-VAL-001: Un undo que no puede resolverse no cambia nada

**Descripción**: si el agente indicado no existe en el timeline, el periodo no es válido o no hay nada que deshacer en el alcance pedido, el repo queda igual y el usuario recibe el motivo.

**Criticidad**: Media

**Regla formal**:
```
IF agente desconocido OR periodo inválido OR conjunto a deshacer vacío
THEN no se modifica el repo + se informa el motivo
```

**Ejemplo**:
- `raptor undo --agent claude-9 --since 20m` sin ese agente en el timeline → "no hay operaciones de claude-9 en los últimos 20 minutos"; nada cambia.
- `raptor undo --since veinte` → periodo no válido; nada cambia.

---

## 2. Cálculos de Negocio

No aplica.

## 3. Reglas de Elegibilidad

No aplica.

---

## 4. Workflows y Estados

### BR-TMC-WF-001: Undo y redo

**Descripción**: `undo` devuelve el ámbito afectado al estado previo a la última operación. `redo` revierte el último undo. Ambos son operaciones de la Time Machine y llevan su propio snapshot previo (BR-TMC-CONS-001), así que nada se pierde al encadenarlos.

**Ámbito por defecto** (D-TMC-19, D-TMC-20, D-TMC-23): `raptor undo` sin flags deshace la última operación del **worktree desde el que se invoca**. Si esa operación es de un actor distinto del solicitante, aplican BR-TMC-AUTH-001 y la regla de solape (BR-TMC-CONS-005). Nunca actúa sobre otros worktrees sin pedirlo.

**Undos seguidos** (TQ-9, D-TMC-25): cada worktree tiene su pila. Un segundo `undo` deshace la operación anterior aún no deshecha, y así sucesivamente. `redo` rehace el último undo; una operación nueva en el worktree invalida el redo pendiente.

**Redos seguidos** (D-TMC-26): el redo está disponible si desde el último undo pendiente no hubo ninguna operación nueva en el worktree. Un redo no cuenta como operación nueva, así que redos seguidos rehacen, en orden inverso, los undos seguidos. Una operación nueva invalida todos los redos pendientes, y el solicitante recibe ese motivo, distinto de "no hay nada que rehacer". Un undo interrumpido cuenta como operación nueva.

**Criticidad**: Alta

**Regla formal**:
```
ámbito = worktree desde el que se invoca                       (D-TMC-19, D-TMC-23)
IF actor de la última operación ≠ solicitante → BR-TMC-AUTH-001 + BR-TMC-CONS-005
undo:  snapshot previo → restaurar estado anterior a la última operación del ámbito aún no deshecha → registrar el undo
       (undos seguidos = pila por worktree, TQ-9)
redo:  disponible si desde el último undo pendiente no hubo una operación nueva en el ámbito   (D-TMC-26)
       → snapshot previo → restaurar el estado anterior a ese undo
       redos seguidos = rehacen los undos seguidos en orden inverso; un redo no es operación nueva
IF hubo una operación nueva en el ámbito después del undo THEN ningún redo disponible + se informa ese motivo   (TQ-9, D-TMC-26)
IF nunca hubo un undo pendiente THEN "nada que rehacer"
IF desde el undo hubo cambios de otro actor en los mismos archivos o ramas
THEN aplica BR-TMC-CONS-005 (solape): se detiene                  (S5, aceptado 2026-10-03; D-TMC-27)
```

**Ejemplo**: un agente hace `reset --hard` y borra 3 archivos sin commitear → `raptor undo` los recupera → `raptor redo` vuelve al estado tras el reset; el contenido de los 3 archivos sigue recuperable en el timeline.

**Ejemplo (pila, TQ-9)**: en `feat-pagos` hubo un commit A y luego un checkout B → `raptor undo` deshace B → otro `raptor undo` deshace A. Si después se hace un commit C, `raptor redo` ya no está disponible.

**Ejemplo (redos seguidos, D-TMC-26)**: tras deshacer B y A, `raptor redo` rehace A y otro `raptor redo` rehace B; `feat-pagos` queda como estaba antes del primer undo. Un tercer `raptor redo` responde que no hay nada que rehacer.

### BR-TMC-WF-002: Undo por agente y por periodo

**Descripción**: `raptor undo --agent <id> [--since <duración>]` deshace, como un solo undo, solo las operaciones del worktree desde el que se invoca con atribución vigente al agente `<id>` en el periodo. Lo "sin atribuir" y lo de otros agentes nunca entra.

**Atribución vigente** (Q50 de motor-local, que cerró P17 el 2026-10-27; D-TMC-22 cumplida): cuenta la atribución en el momento de planificar el undo. Una corrección vigente lleva la sesión al agente corregido; retirarla devuelve la sesión y sus eventos a la atribución detectada. Lo que el undo deshizo queda registrado y una corrección posterior no lo cambia (D-TMC-18).

**Qué elige y qué deshace** (D-TMC-31 a D-TMC-35, 2026-10-27; D-TMC-36, propuesta del PO pendiente de confirmar):
- `<id>` es un id de sesión (`raptor sessions --json`), que elige esa sesión, o un nombre de agente (`claude-code`, o el nombre declarado de un "otro agente"), que elige toda sesión del worktree cuya atribución vigente lo lleva, igual que `raptor timeline --agent`. `unattributed` no vale (D-TMC-32).
- Sin `--since`, el periodo es la retención (30 días); el resultado dice desde cuándo contó (D-TMC-33).
- Cada archivo, rama y `HEAD` que las operaciones del agente cambiaron vuelve al valor que tenía antes de la primera de ellas que lo cambió; todo lo demás queda como está. Lo que el agente cambió con un commit, un merge o un rebase es el contenido que el commit registra: sale del historial, del índice y de los archivos (D-TMC-31).
- Las ediciones sin commitear del agente cuentan como "sin agente" en el MVP (D-TMC-27): no se deshacen, y si están en un archivo que el undo cambiaría, lo detienen por solape (D-TMC-31).
- Si otro actor movió la rama después del primer cambio del agente (un commit intercalado), el undo se detiene por solape: el MVP no reescribe historial. Lo de otro actor anterior al primer cambio del agente se conserva (D-TMC-34).
- Mientras un undo por agente esté activo (no rehecho), `raptor undo` y `--since` no bajan por debajo de él: se rechazan con su motivo y se ofrecen `raptor redo` o restaurar un punto (D-TMC-35).
- Si el agente no tiene nada que deshacer en el worktree desde el que se invoca, nada cambia y el motivo lo dice; el undo por agente nunca actúa sobre otro worktree (D-TMC-36).

**Criticidad**: Alta

**Regla formal**:
```
ámbito = worktree desde el que se invoca                                              (D-TMC-36)
duración = --since, o la retención (30 días) si falta                                   (D-TMC-33)
conjunto = operaciones del ámbito con atribución vigente al planificar = <id> AND momento ≥ ahora − duración   (Q50, D-TMC-32)
EXCLUDE "sin atribuir", otros agentes, eventos en huecos, ediciones sin commitear         (D-TMC-31)
IF conjunto vacío THEN "nada que deshacer en este worktree"; el repo no cambia            (D-TMC-36)
destino(archivo | rama | HEAD) = valor antes del primer cambio del conjunto que lo tocó   (D-TMC-31)
IF otro actor cambió después un sujeto del destino, o movió la rama tras el primer cambio del agente
   → BR-TMC-CONS-005: detener; el repo no cambia                                         (D-TMC-34)
ELSE snapshot previo → deshacer el conjunto → registrar el undo (barrera para undo y --since hasta el redo)   (D-TMC-35)
```

**Ejemplo**: en `feat-login`, la sesión claude-1 de Claude Code hizo dos commits en los últimos 20 minutos, uno con su edición de `api.rs` → `raptor undo --agent <sesión de claude-1> --since 20m` (o `--agent claude-code` si es la única sesión de Claude Code allí) revierte los dos commits y la edición de `api.rs`; un cambio "sin atribuir" en `README.md` queda intacto.

**Ejemplo (D-TMC-34, D-TMC-35)**: si claude-2 hizo un commit en `feat-login` entre los dos de claude-1, el undo se detiene por solape en la rama y nada cambia. Tras un undo por agente que nadie rehízo, un `raptor undo` que alcanzaría una operación anterior se rechaza y ofrece `raptor redo` o restaurar un punto.

**Undo por periodo sin `--agent`** (US-TMC-010, D-TMC-29): `raptor undo --since <duración>` deshace, como un solo undo, todas las operaciones del worktree desde el que se invoca ocurridas en el periodo, de cualquier actor. Lo de otro actor nunca se excluye en silencio: aplica BR-TMC-AUTH-001 a cada operación del conjunto. Un solicitante "sin atribuir" necesita la confirmación interactiva si el conjunto incluye trabajo de un agente (en Windows y por MCP, rechazo); un solicitante agente recibe un rechazo si el conjunto incluye trabajo de otro actor. Si una operación del periodo no se puede deshacer, se rechaza la petición entera. Para deshacer solo lo de un agente está `--agent`.

```
conjunto (--since sin --agent) = operaciones del worktree con momento ≥ ahora − duración, de cualquier actor   (D-TMC-29)
FOR EACH operación del conjunto → BR-TMC-AUTH-001 (confirmación o rechazo; nunca exclusión silenciosa)
IF alguna operación del conjunto no se puede deshacer THEN rechazo entero, el repo no cambia
un solo undo → un solo snapshot previo → un redo lo rehace entero
```

**Ejemplo (D-TMC-29)**: en `feat-login` hubo una operación "sin atribuir" y un commit de claude-1 en los últimos 20 minutos → `raptor undo --since 20m` pide confirmar que se deshará el trabajo de claude-1; sin confirmación, nada cambia.

### BR-TMC-WF-003: Restaurar a un punto del timeline

**Descripción**: el desarrollador elige un snapshot del timeline y el repo vuelve a ese estado. Solo se puede restaurar a un punto que tenga snapshot completo. La restauración lleva snapshot previo, así que se puede deshacer.

**Trabajo de otro actor** (D-TMC-30, 2026-10-27; US-TMC-024): restaurar retira todo lo posterior al punto dentro de su alcance, así que, antes de cambiar nada, el solicitante recibe el plan: el trabajo de otro actor que se perderá, con su actor y su momento. Ese trabajo solo se retira con la confirmación interactiva de BR-TMC-AUTH-001, que cubre exactamente lo listado; un solicitante agente nunca confirma y recibe un rechazo, y en Windows y por MCP también se rechaza. Una edición sin commitear en un worktree del conjunto distinto de aquel donde se pide cuenta como trabajo de otro actor ("sin agente"); en el worktree donde se pide, es del solicitante si este queda "sin atribuir" y de otro actor si es un agente (D-TMC-27). Si al aplicar aparece un cambio posterior que no estaba en el plan, o no se puede comprobar que no apareció ninguno, la restauración se detiene, el repo no cambia y no se ofrece continuar (D-TMC-27); pedirla de nuevo da un plan nuevo. Lo retirado se recupera deshaciendo la restauración (NFR-01).

**Criticidad**: Alta

**Regla formal**:
```
REQUIRE punto con snapshot completo
alcance = worktree donde se pide + ramas y worktrees cambiados después del punto   (D-TMC-20)
plan = trabajo posterior al punto que se retirará, con actor y momento                         (D-TMC-30)
edición sin commitear en un worktree del conjunto ≠ W → trabajo de otro actor ("sin agente")
IF el plan incluye trabajo de otro actor THEN BR-TMC-AUTH-001 (confirmación del plan mostrado; agente, Windows, MCP: rechazo)
snapshot previo → restaurar → registrar la restauración
IF al aplicar aparece un cambio posterior fuera del plan THEN detener; el repo no cambia; sin continuar   (D-TMC-27, D-TMC-30)
IF no se puede comprobar que no apareció ninguno THEN detener (motivo propio); el repo no cambia
```

**Ejemplo (D-TMC-30)**: después de las 10:00 claude-1 hizo un commit en `feat-login` → `raptor restore` al punto de las 10:00, pedido "sin atribuir" en macOS, lista ese commit y pide confirmarlo; sin confirmación, nada cambia. Si claude-1 edita `api.rs` en `feat-login` entre la confirmación y la aplicación, la restauración se detiene y muestra ese cambio. Si quien pide es claude-2, se rechaza sin pedir confirmación.

---

## 5. Permisos y Autorizaciones

### BR-TMC-AUTH-001: Quién puede deshacer qué

**Descripción** (D-TMC-17, D-TMC-23): GitRaptor no puede probar que una petición viene del humano: un agente puede lanzar `raptor undo` desde su propia shell. Por eso el **solicitante** de un undo, redo o restauración se atribuye igual que los eventos: "agente X" o "sin atribuir", nunca "humano" (Q34). Si el solicitante es un agente, solo deshace operaciones con atribución vigente a él. Si es "sin atribuir", deshacer trabajo de otro actor exige una confirmación interactiva del desarrollador en ese momento, que un agente no puede dar.

**Excepciones del MVP** (D-TMC-23 actualizada): en **Windows** no se ofrece esa confirmación hasta que exista una forma fiable de probar que no la da un agente; la petición "sin atribuir" que toca trabajo de otro actor se rechaza con su motivo (TQ-14). En macOS y Linux sigue la confirmación. En la Fase 2 se planea la presencia verificada por el sistema operativo (Touch ID, Windows Hello, polkit). Por **MCP**, una petición "sin atribuir" se rechaza siempre, antes de evaluar qué toca (TQ-7): es más estricto que la regla general, no la contradice. Guardrails puede restringirlo más, nunca ampliarlo. Cómo se identifica al solicitante (CLI frente a MCP) lo decide el Arquitecto.

**Criticidad**: Alta

**Regla formal**:
```
solicitante ∈ {agente X, "sin atribuir"}                       (nunca "humano", Q34)
IF solicitante = agente X
  THEN permitido solo si todo el conjunto tiene atribución vigente = X; si no, rechazo
IF solicitante = "sin atribuir" AND la petición llega por MCP
  THEN rechazo, siempre                                             (TQ-7)
IF solicitante = "sin atribuir" AND el conjunto incluye trabajo de otro actor
  IF sistema = Windows THEN rechazo, sin confirmación               (TQ-14, MVP)
  ELSE pedir confirmación interactiva en ese momento; sin ella, rechazo
IF una política de Guardrails lo prohíbe THEN rechazo
rechazo → el repo no cambia + se informa el motivo
```

**Ejemplo**:
- claude-1 pide vía MCP deshacer un commit de claude-2 → rechazado.
- Claude Code ejecuta `raptor undo` desde su shell y se le atribuye la petición → solo puede deshacer lo suyo.
- Una petición "sin atribuir" en la terminal de macOS o Linux toca un commit de claude-1 → se pide confirmación interactiva; sin ella, nada cambia.
- La misma petición en Windows → rechazada con el motivo ("en Windows no se puede confirmar trabajo de otro actor todavía"); nada cambia.
- Una petición "sin atribuir" llega por MCP, aunque solo toque trabajo "sin atribuir" → rechazada; nada cambia.

---

## 6. Reglas de Consistencia de Datos

### BR-TMC-CONS-001: Sin snapshot previo no hay operación

**Descripción**: toda operación lanzada por GitRaptor (CLI, TUI/Cockpit, MCP) que modifica el repo, y todo undo, redo o restauración, va precedida de un snapshot. Si el snapshot no se completa, la operación no se ejecuta y se informa del motivo. Deriva de NFR-01.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH operación lanzada por GitRaptor que modifica el repo:
  snapshot previo completo → ejecutar
  IF falla el snapshot THEN no ejecutar + informar
```

El previo tiene un plazo por avance: 10 s sin avanzar o 100 s en total desde que se aceptó la petición (D-TMC-42); mientras se guarda, pasado 1 s, la CLI y la TUI lo muestran (D-TMC-43).

**Ejemplo**: el Cockpit descarta el worktree `feat-pagos` y su rama (BR-07) → antes queda un snapshot con su trabajo sin commitear; si el disco está lleno y el snapshot falla, el worktree no se borra.

### BR-TMC-CONS-002: Qué contiene un snapshot

**Descripción**: un snapshot incluye el working tree sin commitear, lo preparado para el próximo commit, los archivos nuevos sin seguimiento y el estado de ramas y worktrees. No incluye (D-TMC-16, actualizada por TQ-16; D-TMC-25):
- los archivos ignorados por `.gitignore`;
- por defecto, la lista cerrada de archivos de credenciales sin seguimiento: `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `.netrc`, `*.tfstate*`, `credentials*.json`. El perfil permite incluirlos;
- los repos anidados sin seguimiento (un directorio con su propio repo dentro del worktree) (TQ-15).

Las credenciales excluidas y los repos anidados se **declaran** en el snapshot como exclusión. Al restaurar, nada de lo excluido se escribe ni se borra.

**Criticidad**: Alta

**Regla formal**:
```
contenido = working tree + preparado + sin seguimiento + estado de ramas y worktrees
EXCLUDE ignorados por .gitignore
EXCLUDE lista cerrada de credenciales sin seguimiento   UNLESS el perfil los incluye   (TQ-16)
EXCLUDE repos anidados sin seguimiento                                                  (TQ-15)
credenciales excluidas y repos anidados → declarados como exclusión en el snapshot
restaurar → lo excluido no se escribe ni se borra
```

**Ejemplo**: un agente crea `nuevo.rs` sin añadirlo y edita `lib.rs` → ambos se recuperan. `node_modules/` y `.env`, ignorados, no se copian ni se tocan al restaurar.

**Ejemplo (credenciales, TQ-16)**: un agente crea `deploy.pem` y `.env.local` sin seguimiento y sin ignorar, junto a `nuevo.rs` → el snapshot guarda `nuevo.rs` y declara `deploy.pem` y `.env.local` como "excluidos por credenciales". Al restaurar ese punto, `nuevo.rs` vuelve y los dos archivos de credenciales quedan como estén en el disco. Si el desarrollador los incluye en su perfil, el siguiente snapshot sí los guarda.

### BR-TMC-CONS-003: Cobertura declarada en dos niveles

**Descripción** (D-TMC-9, D-TMC-10; refina BR-08 y NFR-01 del BRD): (a) **Garantizada**: snapshot previo antes de toda operación lanzada por GitRaptor y de cada undo o restauración. (b) **Por observación**: el trabajo hecho con Git crudo o en el editor se captura a medida que el motor observa cambios, y con snapshot previo cuando existan los hooks de Guardrails, sin depender de ellos. Cada punto del timeline indica su nivel. La Time Machine no promete un snapshot previo donde no lo hay.

**Límites de disco** (TQ-5, D-TMC-25; cifras propuestas 50 MB, 20 GB y máx(5 GB, 5 %), ajustadas por el spike): en la captura por observación, un archivo que supera el tope de tamaño no se copia y la captura queda **parcial**, con la lista de lo omitido. Si se alcanza la cuota del repo o el espacio libre mínimo, la captura por observación se detiene y el timeline muestra un **hueco "sin espacio"** (BR-TMC-EDGE-002). El snapshot previo garantizado (nivel a) no tiene tope por archivo.

**Previo vía hooks de Guardrails** (US-TMC-005; Decisión del orquestador (2026-10-04), validada por Arquitecto/PO): con los hooks de Guardrails, el rebase y el borrado de una rama local con Git crudo tienen un punto `previo_hook` tomado antes de que Git toque nada. Si no se pudo guardar, la operación no figura como "snapshot previo" y, hasta US-GRD-017, el hook avisa y deja pasar. `checkout -f`, `restore` y `reset --hard` con Git crudo no tienen un hook previo (ADR-GRD-002) y siguen cubiertos por observación: R2 queda abierto para ellos. ⚠️ **ASSUMPTION** (cifras a revisar con uso real): cupo de `previo_hook` de 10 por minuto y 120 en 24 h por solicitante y worktree, 300 en 24 h por worktree y 1.000 en 24 h por repo; los cupos por worktree y por repo solo cuentan a los agentes (Q-GRD-37). Un cupo agotado se declara en el aviso como `quota-exceeded` y nunca borra un punto.

**Criticidad**: Alta

**Ejemplo**: un agente ejecuta `git reset --hard` con Git crudo sin hooks de Guardrails → el timeline muestra el último estado capturado antes del reset como "capturado por observación", no como "snapshot previo". Lo editado entre esa captura y el reset puede perderse (riesgo R2 del contexto).

### BR-TMC-CONS-004: Las escrituras de la Time Machine son suyas, explícitas y recuperables

**Descripción**: la Time Machine solo escribe para guardar snapshots y para ejecutar un undo, redo o restauración pedido por un actor. Los snapshots no aparecen como cambios del usuario, no se empujan al remoto por accidente, no los borra un `git gc` y no los altera un agente que trabaja en el working tree (D-TMC-11). Ninguna escritura se atribuye al Motor local (Q21).

**Excepción explícita** (ADR-TMC-003 § 6.4, D-TMC-25): al arrancar tras una interrupción, la Time Machine libera el bloqueo de Git que ella misma dejó, sin que nadie lo pida, para que Git no quede bloqueado para el usuario y sus agentes. No toca contenido, y un bloqueo que no es suyo nunca se libera.

**Criticidad**: Alta

**Regla formal**:
```
escrituras permitidas = {guardar snapshot, undo, redo, restauración pedida}
                      + {liberar al arrancar un bloqueo de Git propio}       (excepción, no toca contenido)
bloqueo ajeno → nunca se libera
Constraint: guardar un snapshot no cambia el estado observable del repo
Constraint: push / gc / trabajo de un agente no exponen ni alteran snapshots
Constraint: "agente" = Git y archivos del repo y sus worktrees (incl. borrar/podar worktrees y mantenimiento);
            escribir en la carpeta de datos de GitRaptor no se impide; la alteración de un snapshot se detecta   (D-TMC-40)
Constraint: un snapshot no depende de worktree, rama, commits ni alternates del repo;
            dentro de la retención solo lo retira la purga (edad, tope o cuota; aviso visto y 24 h)              (D-TMC-41)
```

### BR-TMC-CONS-005: Atribución vigente, presentación y solape

**Descripción**: el timeline y el undo por agente usan la atribución vigente (Q37). "Sin atribuir" es la categoría del modelo y el valor del cable; al usuario se le presenta como "sin agente" ("cambios tuyos o de otra herramienta") y nunca como "humano" (Q34, D-TMC-12; enmienda 2026-10-08 validada por PO, alineada con el design system). Si deshacer lo de un actor tocaría cambios posteriores de otro actor en los mismos archivos o fragmentos, la Time Machine no sobrescribe: se detiene, muestra el solape y deja decidir al desarrollador (D-TMC-13). El registro de un undo (el solicitante, atribuido según BR-TMC-AUTH-001, y sobre qué actuó) no se reescribe si después cambia la atribución (D-TMC-18).

**Alcance en el MVP** (D-TMC-27, 2026-10-27):
- Tras un solape, la única decisión disponible es no seguir: el undo o el redo no se ejecuta y el repo no cambia. Continuar a pesar del solape o fusionar por fragmentos queda para después del MVP.
- El solape se mide por archivo y por rama: un cambio posterior de otro actor en cualquier parte del mismo archivo, o un movimiento posterior de la misma rama, detiene el undo. La comparación por fragmento llega más adelante (TQ-6).
- Solo cuentan los cambios posteriores al cambio que se deshace. Un archivo o una rama que el cambio deshecho no tocó y que otro actor cambió después se deja como está y se informa.
- Las ediciones de archivos sin evento de Git no tienen actor en el MVP y cuentan como "sin agente", aunque las haya hecho un agente. Por eso, si un agente edita sin Git el mismo archivo después de su propia operación, deshacer esa operación también se detiene por solape.
- Si no se puede separar el cambio deshecho de lo posterior, el undo se detiene igual, con ese motivo: nunca se escribe a ciegas (NFR-01).
- La restauración, que por definición retira todo lo posterior al punto, aplica el solape a su manera: lista el trabajo de otro actor y lo retira solo con confirmación, y se detiene ante lo que no estaba en su plan (D-TMC-30, BR-TMC-WF-003).

**Criticidad**: Alta

**Regla formal**:
```
actor mostrado ∈ {"agente X (detectado|registrado)", "sin agente"}
IF solape con otro actor THEN detener + mostrar solape; el repo no cambia
MVP: tras el solape no se continúa                                     (D-TMC-27)
MVP: granularidad = archivo y rama; edición sin evento de Git = "sin agente"
archivo o rama que solo cambió otro actor después → se conserva y se informa
IF no se puede separar lo deshecho de lo posterior THEN detener (motivo propio); el repo no cambia
registro de undo = inmutable; atribución mostrada = vigente
```

**Ejemplo**: claude-1 commiteó una edición de `api.rs` a las 10:00 y un cambio "sin atribuir" tocó la misma función a las 10:05 → el undo por agente de claude-1 se detiene y muestra ambos cambios.

---

## 7. Reglas de Tiempo y Expiración

### BR-TMC-TIME-001: Retención de snapshots

**Descripción** (D-TMC-15): los snapshots se conservan un tiempo configurable; por defecto 30 días. El valor lo admiten el perfil y la configuración local personal, no la del equipo (Q24). Nunca se purga el snapshot previo a la última operación destructiva. Se avisa antes de purgar: la purga solo ocurre si el aviso se mostró al menos una vez en la CLI o la TUI **y** pasaron 24 horas desde que se mostró por primera vez; si nadie lo vio, no se purga (TQ-11). ~~Alcanzar una cuota de disco no adelanta la purga (TQ-5).~~ **Enmienda (2026-10-05, decisión de Rene Bonilla)**: hay un tope de disco total de la Time Machine, configurable solo en el perfil (`timeMachine.maxDiskSizeGiB`, 10 GiB por defecto). Al pasar el 80 % se avisa, y tras el mismo aviso visto y las 24 horas se purgan los puntos más antiguos no protegidos, de 7 días o más, hasta bajar al 70 %. Al 100 % antes de la gracia se detiene la captura por observación, sin borrar nada (US-TMC-022, ADR-TMC-007). Borrar contenido concreto de los snapshots (`raptor tm forget`) queda fuera del MVP (D-TMC-24, TQ-17). **Precisión (2026-10-28)**: la cuota por repo purga con la misma regla que el tope total (D-TMC-38, TQ-24 → a, abierta a veto de Rene). Un aviso se ha **mostrado** cuando se pintó en la CLI con terminal y sin `--json`, o en la TUI; que un agente lo lea con `--json`, sin terminal o por MCP no arranca la gracia (D-TMC-38).

**Criticidad**: Media

**Regla formal**:
```
retención = local personal ?? perfil ?? 30 días        (equipo: no admitido)
purgable = snapshot más antiguo que la retención AND NOT previo a la última operación destructiva
purga solo si aviso mostrado en CLI/TUI AND ≥ 24 h desde la primera vez que se mostró   (TQ-11)
tamaño ≥ 80 % del tope → aviso; candidatos = más antiguos AND NOT protegidos AND edad ≥ 7 días, hasta el 70 %   (2026-10-05)
purga por tamaño solo si aviso mostrado en CLI/TUI AND ≥ 24 h desde la primera vez que se mostró
tamaño ≥ 100 % del tope antes de la gracia → se detiene la captura por observación, no se purga
"mostrado" = pintado en la CLI con terminal y sin --json, o en la TUI; MCP, --json o sin terminal no cuentan   (D-TMC-38)
pasada de purga: al arrancar, cada 24 h, ≤ 1 min tras vencer una gracia, al pasar del 80 % del tope o de la cuota   (D-TMC-37)
configuración del perfil o local ignorada por un diagnóstico → la pasada no anuncia ni purga               (D-TMC-37)
se purga solo lo que anunció un aviso con gracia vencida; lo nuevo, con su propio aviso                    (D-TMC-38)
cuota del repo ≥ 80 % → aviso del repo; purga hasta el 70 % de la cuota, puntos ≥ 7 días no protegidos     (D-TMC-38, TQ-24 → a)
tope total ≥ 80 % → un solo aviso para todos los repos; verlo en cualquiera arranca la gracia de todos      (D-TMC-38)
al purgar: IF el punto ya no cumple THEN no se purga y sigue disponible                                     (D-TMC-38)
tope o cuota al 100 % → se detienen la observación y el manual (con causa y salidas); los previos se toman  (D-TMC-39)
repos que ya no se observan: cuentan para el tope, no se purgan solos, se nombran aparte                   (D-TMC-39, TQ-25)
```

**Ejemplo (TQ-11)**: un snapshot cumple 30 días el lunes, pero el desarrollador no abre la CLI ni la TUI hasta el miércoles → el miércoles ve el aviso y la purga ocurre a partir del jueves a la misma hora.

**Ejemplo (D-TMC-38)**: el lunes se anuncian 40 puntos de más de 30 días y el desarrollador ve el aviso en `raptor status` a las 10:00. El martes a las 10:01 se purgan esos 40. Los 3 puntos que cumplieron 30 días el lunes por la tarde no entran: llevan su propio aviso y se purgan 24 h después de que se vea. Si a las 9:00 del lunes un agente había leído `raptor status --json`, eso no contó.

**Ejemplo (TQ-24 → a)**: cuota de 5 GiB, tope de 10 GiB y un solo repo con 4,1 GiB: la CLI y la TUI avisan de que el repo pasó el 80 % de su cuota, con cuántos puntos de 7 días o más se purgarán, de qué periodo y cuánto espacio se libera.

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-TMC-EDGE-001: Lo deshecho ya está en el remoto

**Descripción**: el undo es solo local. Si lo deshecho ya se empujó, se avisa de que sigue en el remoto. La Time Machine nunca hace push ni force-push por su cuenta (D-TMC-14).

**Criticidad**: Alta

### BR-TMC-EDGE-002: Huecos de observación en el timeline

**Descripción**: el timeline muestra cada hueco de forma explícita. Lo ocurrido en un hueco es "sin atribuir" (BR-EDGE-005 de motor-local), nunca entra en un undo por agente y no ofrece puntos de restauración que no tengan snapshot. Un hueco puede deberse a falta de espacio: la captura por observación se detuvo por la cuota o el espacio libre mínimo, y el hueco se declara como "sin espacio" (TQ-5, BR-TMC-CONS-003). El hueco "sin espacio" lo causa el tope total de la Time Machine, la cuota del repo o el espacio libre mínimo, y lo dice; en él siguen tomándose los snapshots previos de las operaciones protegidas (D-TMC-39).

**Criticidad**: Alta

### BR-TMC-EDGE-003: Interrupción a mitad de un snapshot o de un undo

**Descripción**: si el proceso muere durante un snapshot, ese snapshot no cuenta como punto válido y el repo no cambió. Si muere durante un undo o restauración, el repo queda recuperable al estado previo gracias al snapshot que lo precede, y al volver se informa de lo ocurrido (NFR-12).

**Fallo detectado a mitad** (TQ-10, D-TMC-25): si el undo o la restauración detecta un fallo antes de terminar (disco lleno, un archivo bloqueado por otro programa), no hay vuelta atrás automática: la operación se detiene, queda marcada como **interrumpida** en el timeline, igual que tras una muerte del proceso, y se informa de que `raptor undo` devuelve el repo al snapshot previo. Hay un único camino de recuperación.

**Criticidad**: Alta

### BR-TMC-EDGE-004: Operación de Git en curso

**Descripción**: (S6, aceptado por Rene Bonilla el 2026-10-03). Con un rebase o un merge a medias, el undo y la restauración se detienen y piden terminar o abortar esa operación antes; el repo no cambia. El aviso nombra la operación en curso y el worktree donde está. Lo mismo vale para un cherry-pick, un revert, un bisect o una secuencia a medias.

**Otros worktrees** (D-TMC-28, 2026-10-27): una operación de Git en curso en un worktree que el undo no toca no bloquea el undo, salvo que esa operación use una rama que el undo movería; entonces el undo se detiene porque la rama está en uso y la rama no se mueve. La restauración, que puede mover todas las ramas, se sigue deteniendo con una operación en curso en el worktree principal.

**Tras abortar**: el abort es una operación más del worktree si la Time Machine la vio. El undo que se pide después corre y dice qué deshizo; con undos seguidos se vuelve al estado anterior a la operación abortada sin pérdida.

**Regla formal**:
```
IF operación en curso en un worktree que el undo o la restauración toca THEN detener + aviso (operación, worktree); el repo no cambia
IF operación en curso en otro worktree AND usa una rama que el undo movería THEN detener (rama en uso); el repo no cambia   (D-TMC-28)
IF operación en curso en otro worktree AND no usa ninguna rama del undo THEN el undo procede                             (D-TMC-28)
restauración: una operación en curso en el worktree principal la detiene siempre
```

**Ejemplo (D-TMC-28)**: Rene tiene un merge a medias en `main` y claude-1 pide deshacer su último commit en `feat-x` → el undo procede, porque no toca `main`. Si claude-2 está rebasando la rama que el undo movería, el undo se detiene y la rama no se mueve.

**Criticidad**: Media

---

## Decisiones de negocio

D-TMC-1 a D-TMC-25 están en [context.md § Decisiones tomadas](./context.md#decisiones-tomadas). D-TMC-26 a D-TMC-29 salen de las Dev Specs de US-TMC-003, US-TMC-010, US-TMC-012 y US-TMC-015; D-TMC-30, de la historia del solape de la restauración (TQ-18, US-TMC-024); D-TMC-31 a D-TMC-35, de la Dev Spec de US-TMC-011; D-TMC-36, de su validación por el PO; D-TMC-37 y D-TMC-38, de la Dev Spec de US-TMC-016; D-TMC-39, de la de US-TMC-022; D-TMC-40 y D-TMC-41, de la de US-TMC-018, y D-TMC-42 y D-TMC-43, de la de US-TMC-020.

| # | Decisión | Fuente | Fecha | Decidido por | Reglas afectadas |
|---|----------|--------|-------|--------------|------------------|
| D-TMC-26 | (TQ-9) El redo está disponible si desde el último undo pendiente no hubo ninguna operación nueva en el worktree; un redo no es una operación nueva. Redos seguidos rehacen, en orden inverso, los undos seguidos. Una operación nueva invalida todos los redos pendientes y el solicitante recibe ese motivo, distinto de "nada que rehacer". Un undo interrumpido cuenta como operación nueva. | DS-US-TMC-003 D1, D2 | 2026-10-27 | Orquestador, validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-001 |
| D-TMC-27 | (D-TMC-13, TQ-6) En el MVP, un undo o un redo detenido por solape no se continúa: la única decisión es no seguir y el repo no cambia. El solape se mide por archivo y por rama, y las ediciones sin evento de Git cuentan como "sin agente", aunque las haya hecho un agente. Continuar a pesar del solape o fusionar por fragmentos queda para después del MVP. | DS-US-TMC-012 D3, D11 | 2026-10-27 | Orquestador, validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-CONS-005, BR-TMC-WF-001 |
| D-TMC-28 | Una operación de Git en curso en un worktree que el undo no toca no bloquea ese undo, salvo que la operación use una rama que el undo movería: entonces se detiene por rama en uso. La restauración se sigue deteniendo con una operación en curso en el worktree principal. El aviso nombra la operación y el worktree. | DS-US-TMC-015 D1, D2, D3 | 2026-10-27 | Orquestador, validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-EDGE-004 |
| D-TMC-29 | `raptor undo --since` cuenta todas las operaciones del worktree en el periodo, de cualquier actor, como un solo undo. Lo de otro actor nunca se excluye en silencio: un solicitante "sin atribuir" necesita la confirmación interactiva para el trabajo de un agente (rechazo en Windows y por MCP) y un solicitante agente recibe un rechazo. Para excluir lo de otros está `--agent` (US-TMC-011). | DS-US-TMC-010 D1, D4 | 2026-10-27 | Orquestador, validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-002, BR-TMC-AUTH-001 |
| D-TMC-30 | (TQ-18, D-TMC-13) La restauración retira, con la confirmación interactiva de BR-TMC-AUTH-001, el trabajo de otro actor que lista su plan: antes de cambiar nada, el solicitante ve qué trabajo de otro actor se perderá, con su actor y su momento, y lo confirma; el snapshot previo lo deja recuperable (NFR-01). Se detiene ante lo que no está en el plan: un cambio posterior aparecido entre el plan y la aplicación, o la imposibilidad de comprobar que no apareció ninguno, detiene la restauración sin cambiar el repo y sin opción de continuar (D-TMC-27). Una edición sin commitear en un worktree del conjunto distinto de aquel donde se pide cuenta como trabajo de otro actor ("sin agente"); en el worktree donde se pide, es del solicitante si este queda "sin atribuir" y de otro actor si es un agente, aunque la haya hecho ese mismo agente, así que un agente nunca restaura un worktree con ediciones sin commitear. Un solicitante agente nunca confirma: si el plan incluye trabajo de otro actor, recibe un rechazo; en Windows y por MCP también se rechaza (D-TMC-23). | US-TMC-024; DS-US-TMC-012 TQ-18, G1, G5 | 2026-10-27 | Orquestador, validada por PO y Arquitecto; abierta a veto de Rene | BR-TMC-WF-003, BR-TMC-CONS-005, BR-TMC-AUTH-001 |
| D-TMC-31 | Deshacer el trabajo de un agente devuelve cada archivo, rama y `HEAD` que sus operaciones cambiaron al valor que tenía antes de la primera de ellas que lo cambió, y deja todo lo demás como está. Lo que el agente cambió con un commit, un merge o un rebase es el contenido que el commit registra: sale del historial, del índice y de los archivos, aunque el commit haya recogido una edición previa de otro (se recupera con el previo del undo, NFR-01). Las ediciones sin commitear del agente cuentan como "sin agente" en el MVP (D-TMC-27): no entran y, si están en un archivo que el undo cambiaría, lo detienen por solape. `raptor undo` sin flags y `--since` no cambian: siguen volviendo al estado previo de la operación. | DS-US-TMC-011 § Decisiones, G1, G2 | 2026-10-27 | Decisión del orquestador (2026-10-27), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-002, BR-TMC-CONS-005 |
| D-TMC-32 | `--agent <id>` acepta un id de sesión (`raptor sessions --json`), que elige esa sesión, o un nombre de agente (`claude-code`, o el nombre declarado de un "otro agente"), que elige toda sesión del worktree cuya atribución vigente lo lleva, con la misma regla que `raptor timeline --agent`: lo que el timeline muestra para un agente y un periodo es lo que el undo deshace. Fija la sintaxis provisional de D3 de DS-US-TMC-006. `unattributed` no vale para el undo. Un nombre declarado que no se puede escribir en `--agent` (con espacios) se elige por su id de sesión en el MVP (TQ-22). | DS-US-TMC-011 § Decisiones, TQ-22 | 2026-10-27 | Decisión del orquestador (2026-10-27), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-002 |
| D-TMC-33 | `--since` es opcional con `--agent`; sin él, el periodo es la retención (30 días). Con él, `undo --agent X --since D` es `undo --since D` filtrado por actor: un solo undo, un previo, un redo. El resultado dice desde cuándo contó el periodo. | DS-US-TMC-011 § Decisiones; el PO añade que el resultado diga el periodo | 2026-10-27 | Decisión del orquestador (2026-10-27), validada por Arquitecto y PO (ajustada); abierta a veto de Rene | BR-TMC-WF-002 |
| D-TMC-34 | Si otro actor movió la rama después del primer cambio del agente (un commit intercalado), el undo por agente se detiene por solape en la rama, sin cambiar el repo: el MVP no reescribe historial (D-TMC-27). Lo de otro actor anterior al primer cambio del agente no cuenta y se conserva. | DS-US-TMC-011 § Decisiones, D4 | 2026-10-27 | Decisión del orquestador (2026-10-27), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-002, BR-TMC-CONS-005 |
| D-TMC-35 | Mientras un undo por agente esté activo (no rehecho), `raptor undo` y `--since` no bajan por debajo de él, porque devolver un punto anterior traería de vuelta lo que el undo por agente quitó. Se rechazan con su motivo, sin cambiar nada, y se ofrecen `raptor redo` o restaurar un punto (BR-TMC-WF-003). Si la barrera resulta demasiado estricta queda abierta en TQ-21. | DS-US-TMC-011 § Decisiones, TQ-21 | 2026-10-27 | Decisión del orquestador (2026-10-27), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-WF-002, BR-TMC-WF-001 |
| D-TMC-36 | El undo por agente actúa en el worktree desde el que se invoca, como `raptor undo` y `--since` (D-TMC-19, D-TMC-23): nunca toca otro worktree. Si el agente no tiene nada que deshacer allí en el periodo, nada cambia y el motivo dice que es en este worktree. | Validación del PO de DS-US-TMC-011 (§ Diseño, `undo.reason.nothing-of-agent`) | 2026-10-28 | Propuesta del PO (2026-10-28); recoge lo que DS-US-TMC-011 ya diseña (§ Contexto rápido), pendiente de confirmar por el orquestador; abierta a veto de Rene | BR-TMC-WF-002, BR-TMC-WF-001 |
| D-TMC-37 | (BR-TMC-TIME-001) "Corresponde purgar": el trabajo de purga de cada repo corre al arrancar el daemon, cada 24 h y como mucho un minuto después de que venza la gracia de un aviso; por tamaño, también al pasar del 80 % del tope total o de la cuota del repo (US-TMC-022). Cada pasada purga solo lo que anunció un aviso con la gracia vencida y anuncia lo nuevo; sin ningún aviso visto, solo anuncia. Mientras la configuración del perfil o la local del repo esté ignorada por un diagnóstico (PQ-8), la pasada no anuncia ni purga nada: un error en otra clave no puede adelantar una purga (falla cerrado). | DS-US-TMC-016 § Diseño, D1, D13; DS-US-TMC-022 D14 | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-TIME-001 |
| D-TMC-38 | (BR-TMC-TIME-001, TQ-24 → a) Cada aviso de purga cubre un conjunto fijo de puntos: solo esos se purgan cuando vence su gracia; los que pasan a ser candidatos después llevan su propio aviso y sus 24 h. El aviso por edad y el de la cuota son de un repo; el del tope total es uno solo, visible desde cualquier repo observado, lista los puntos de cada repo, y verlo en cualquiera arranca la gracia de todos los que anuncia. Un aviso cuenta como visto solo si se pinta en una terminal (la CLI con terminal y sin `--json`) o en la TUI; leerlo por MCP, con `--json` o sin terminal no arranca la gracia. Si al purgar un punto anunciado ya no cumple (se amplió la retención, pasó a protegido o el tamaño ya bajó del 70 %), no se purga y sigue disponible. El aviso dice cuántos puntos, de qué periodo, cuánto espacio como máximo y desde cuándo se purgarán. Si tras purgar por tamaño se sigue en el 80 % o más, la siguiente ronda lleva su propio aviso y su gracia. La cuota por repo purga con la misma regla que el tope total (aviso al 80 % de la cuota, purga hasta el 70 % de puntos de 7 días o más no protegidos, aviso visto y 24 h); esto cierra TQ-24 y queda **abierto a veto de Rene**, porque cambia su TQ-5 → (b). | DS-US-TMC-016 D2, D9, D12; DS-US-TMC-022 D2, D3, D11 | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; TQ-24 abierta a veto de Rene | BR-TMC-TIME-001 |
| D-TMC-39 | (BR-TMC-TIME-001, BR-TMC-EDGE-002, TQ-25) El tope total y la cuota por repo, al 100 %, detienen la captura por observación y los snapshots manuales, nunca los snapshots previos (garantizado y de hook): estos se toman mientras quepan en el disco y cuentan para la purga. El rechazo de un snapshot manual dice la causa (tope, cuota o poco espacio libre) y las salidas (subir el tope en el perfil o esperar a la purga anunciada, con su fecha). El hueco "sin espacio" empieza con la primera captura omitida, acaba con la siguiente que se toma y dice su causa. Mientras el tope o la cuota estén superados, la CLI y la TUI lo avisan en cada comando. Los repos que ya no se observan cuentan para el tope y no se purgan solos: el aviso persistente y `raptor status` los nombran aparte, con su tamaño, y dicen cómo liberar espacio (volver a observarlos, para que la pasada los purgue con las reglas normales, o subir el tope). Cierra TQ-25. | DS-US-TMC-022 D4, D5, D6, D12, D13 | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-TIME-001, BR-TMC-CONS-003, BR-TMC-EDGE-002 |
| D-TMC-40 | (D-TMC-11) "Un agente no altera los snapshots" cubre todo lo que un agente hace con Git y con los archivos del repo y sus worktrees, incluido borrar un worktree, podarlo (`worktree prune`) y cualquier mantenimiento. No cubre escribir o borrar a propósito en la carpeta de datos de GitRaptor: eso no se impide y queda fuera de esta garantía. La alteración de un snapshot se detecta (su verificación falla y se avisa, SEC-TMC-09); la desaparición entera de la carpeta de un repo no se detecta en el MVP (G4 de DS-US-TMC-018, candidata para INF-TMC-001 § 6 o una historia nueva). | DS-US-TMC-018 § Decisiones, G4 | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-CONS-004 |
| D-TMC-41 | (D-TMC-11) Un snapshot no depende de nada del repo del usuario: sigue disponible aunque desaparezcan su worktree, su rama, sus commits o los objetos que el repo tomaba de otro (`alternates`). Dentro de la retención solo lo retira la purga de la Time Machine, por edad, por tope o por cuota, siempre con aviso visto y 24 h (BR-TMC-TIME-001, D-TMC-38). | DS-US-TMC-018 § Decisiones | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-CONS-004, BR-TMC-CONS-002, BR-TMC-TIME-001 |
| D-TMC-42 | (NFR-04, US-TMC-020 escenario 3) Un snapshot previo que sigue avanzando no se corta: la operación espera mientras la captura avance (cambio de fase, un archivo más escrito o más contenido leído), hasta un tope fijo de 100 s contados desde que el daemon aceptó la petición, con la espera en cola incluida, para que la respuesta llegue antes de que el cliente se rinda. Si la captura pasa 10 s sin avanzar o llega al tope, la operación no se ejecuta, el repo no cambia y el motivo es `prior-timeout` (sin snapshot previo no hay operación, BR-TMC-CONS-001), con dos textos: "guardar el trabajo primero dejó de avanzar; no se ejecutó y nada cambió; reinténtalo" o "el trabajo sin commitear es demasiado grande para guardarlo a tiempo (X MB leídos); no se ejecutó y nada cambió. Si son archivos generados, añádelos al .gitignore: los ignorados no se guardan en los snapshots ni se protegen". Los dos plazos son fijos, sin clave en el perfil, y nunca se sugiere subirlos. La captura tardía queda como punto del timeline, nunca como previo de esa operación. Sustituye el plazo absoluto de 10 s de TS-TMC-004. Cierra TQ-27. | DS-US-TMC-020 § Decisiones, D12 | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-CONS-001 |
| D-TMC-43 | (NFR-04, US-TMC-020 escenario 3; E11 de SPIKE-TMC-001) Si guardar el snapshot previo de una operación pasa de 1 s, la CLI y la TUI muestran que se está guardando el trabajo primero, con el tiempo transcurrido y lo leído, y lo actualizan cada segundo hasta la respuesta. Solo lo ve quien pidió la operación. El MCP no recibe progreso: su respuesta llega al terminar. La CLI no lo muestra con `--json` ni sin terminal, para no cambiar la salida de máquina. Sin porcentaje ni tiempo estimado; el aspecto queda pendiente del design-flow de la Time Machine. | DS-US-TMC-020 § Decisiones | 2026-10-28 | Decisión del orquestador (2026-10-28), validada por Arquitecto y PO; abierta a veto de Rene | BR-TMC-CONS-001 |

### Preguntas técnicas de las Dev Specs (2026-10-28)

| ID | Pregunta | Respuesta | Dueño | Origen | Estado |
|---|---|---|---|---|---|
| TQ-24 | Con los valores por defecto (cuota de 5 GiB, tope de 10 GiB), quien tiene un solo repo llega a la cuota antes del 80 % del tope. ¿La cuota también purga por tamaño? | La cuota purga con la misma regla que el tope, con aviso por repo (D-TMC-38). Abierta a veto de Rene | PO / Rene | DS-US-TMC-022 | **Cerrada → (a)** |
| TQ-25 | La carpeta de un repo que ya no se observa cuenta para el tope y la pasada no puede purgarla. ¿Qué se hace? | Cuenta y no se purga sola; el aviso persistente y `raptor status` la nombran aparte con cómo liberar espacio (D-TMC-39) | PO | DS-US-TMC-022 | **Cerrada** |
| TQ-26 | En Windows, ¿el lector del daemon mantiene abiertos los packs del usuario entre capturas e impide que `git gc`/`repack` borre los viejos? (BR-TMC-CONS-004: guardar no cambia el repo) | Por defecto: el test `user_maintenance_is_not_blocked_by_open_store_or_reader` lo mide; si falla, se sueltan los manejadores al terminar cada captura. Pendiente: etapa de validación multiplataforma (XP-49) | Arquitecto | DS-US-TMC-018 | **Abierta** |
| TQ-27 | ¿100 s es un tope aceptable para el previo de un repo con archivos sin seguimiento muy grandes (unos 14 GB a 145 MB/s)? ¿Qué dice el motivo al agotarse? | 10 s sin avance y 100 s fijos desde la aceptación, sin clave en el perfil; dos textos de `prior-timeout`, el segundo sugiere `.gitignore` y nunca subir el tope (D-TMC-42) | PO + Arquitecto | DS-US-TMC-020 | **Cerrada** |

---

## Matriz de Priorización

| Regla | Criticidad | Prioridad |
|-------|------------|-----------|
| BR-TMC-CONS-001, CONS-002, CONS-004 | Alta | 🔴 P0 |
| BR-TMC-WF-001, EDGE-003 | Alta | 🔴 P0 |
| BR-TMC-CONS-003, CONS-005, WF-002, AUTH-001, EDGE-001, EDGE-002 | Alta | 🟡 P1 |
| BR-TMC-WF-003, VAL-001, TIME-001, EDGE-004 | Media/Alta | 🟡 P1 |

---

## Trazabilidad

### Reglas → User Stories

Pendiente: las historias se generan tras la aprobación humana del requerimiento (ADR-018).

### Reglas → Criterios de Aceptación

Cada regla tendrá al menos un escenario Gherkin, incluido uno negativo, en su historia.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 0.1 | 2026-10-03 | PO (AADD) | Versión inicial en revisión. |
| 0.2 | 2026-10-03 | PO (AADD) | RESERVAS del Artifact Judge: AUTH-001 reformulada como supuesto con solicitante atribuido (P8, P14); ámbito por defecto del undo en WF-001 (S8); ejemplo de CONS-003 precisado. |
| 0.3 | 2026-10-03 | PO (AADD) | Rene Bonilla cierra P1-P14 (D-TMC-10 a D-TMC-23): sin marcas de supuesto en AUTH-001, WF-001 (ámbito), WF-003, CONS-002, CONS-003 y TIME-001. Siguen como supuesto S5 (redo con solape, WF-001) y S6 (EDGE-004). |
| 0.4 | 2026-10-03 | PO (AADD) | Rene Bonilla acepta S5 y S6: sin marcas de supuesto en WF-001 (redo) y EDGE-004. |
| 0.5 | 2026-10-03 | PO (AADD) | Rene Bonilla acepta TQ-1 a TQ-17 del Arquitecto. CONS-002: lista cerrada de credenciales excluida por defecto, con opción en el perfil, y repos anidados excluidos; ambos declarados y no tocados al restaurar (TQ-16, TQ-15; D-TMC-16 actualizada). AUTH-001: en Windows, rechazo sin confirmación en el MVP (TQ-14; D-TMC-23 actualizada) y rechazo siempre por MCP (TQ-7). WF-001: pila de undo por worktree (TQ-9). CONS-003 y EDGE-002: tope por archivo, cuotas y hueco "sin espacio" (TQ-5). CONS-004: excepción de liberar el bloqueo propio al arrancar. TIME-001: aviso visto + 24 h (TQ-11); `forget` fuera del MVP (TQ-17, D-TMC-24). EDGE-003: fallo a mitad sin rollback automático (TQ-10). |
| 0.6 | 2026-10-09 | Orquestador, validada por Arquitecto/PO | US-TMC-005: CONS-003 añade el previo vía hooks de Guardrails (rebase y borrado de rama local), R2 abierto para `checkout -f`/`restore`/`reset --hard`, y los cupos de `previo_hook` como ⚠️ ASSUMPTION (los globales solo cuentan agentes, Q-GRD-37). |
| 0.7 | 2026-10-27 | Orquestador, validada por Arquitecto y PO | Dev Specs de US-TMC-003, US-TMC-010, US-TMC-012 y US-TMC-015: nueva sección "Decisiones de negocio" con D-TMC-26 a D-TMC-29. WF-001: redos seguidos y motivo de la invalidación (D-TMC-26). WF-002: undo por periodo sin `--agent`, de cualquier actor y sin exclusiones silenciosas (D-TMC-29). CONS-005: alcance del MVP del solape, sin continuar, por archivo y por rama, ediciones sin Git como "sin agente" (D-TMC-27). EDGE-004: el aviso nombra la operación y el worktree, una operación en curso en otro worktree no bloquea salvo rama en uso, y el undo tras un abort (D-TMC-28). |
| 0.8 | 2026-10-27 | Orquestador, validada por PO y Arquitecto | Solape de la restauración (TQ-18): nueva US-TMC-024 y D-TMC-30. WF-003: la restauración lista el trabajo de otro actor que retirará y lo retira solo con confirmación (rechazo para un agente, en Windows y por MCP); las ediciones sin commitear de otro worktree del conjunto cuentan como de otro actor; un cambio fuera del plan, o no poder comprobarlo, la detiene sin continuar. CONS-005: remite a WF-003 para la restauración. |
| 0.9 | 2026-10-28 | Orquestador, validada por Arquitecto y PO | Undo por agente (DS-US-TMC-011): D-TMC-31 a D-TMC-35 (D-TMC-33 ajustada por el PO) y D-TMC-36, propuesta del PO pendiente de confirmar. WF-002: ámbito del worktree desde el que se invoca, atribución vigente al planificar con referencia a Q50 de motor-local (P17 cerrada), selector por sesión o por nombre, periodo por defecto de 30 días y el resultado dice desde cuándo, destino por archivo, rama y `HEAD`, ediciones sin commitear como "sin agente", commit intercalado de otro actor como solape sin reescribir historial, y barrera para `undo` y `--since` mientras el undo por agente no se rehaga; ejemplos con sesiones. CONS-005: el ejemplo pasa a una edición commiteada. |
| 0.10 | 2026-10-28 | Orquestador, validada por Arquitecto y PO | US-TMC-024 tras la revisión del Arquitecto: el cambio que detiene la restauración es un commit; el hueco es una pérdida de eventos del observador sin reinicio (con reinicio, la confirmación caduca); D-TMC-30 precisa que un agente nunca restaura un worktree con ediciones sin commitear, ni las suyas. |
| 0.11 | 2026-10-28 | Orquestador, validada por Arquitecto y PO | Dev Specs de US-TMC-016 y US-TMC-022: D-TMC-37 (cuándo corresponde purgar; configuración ignorada no purga), D-TMC-38 (conjunto fijo por aviso; "mostrado" = pintado; cuota por repo con la regla del tope, TQ-24 → a, abierta a veto de Rene; un solo aviso del tope) y D-TMC-39 (tope y cuota detienen la observación y el manual, nunca los previos; rechazo del manual con causa y salidas; repos no observados, TQ-25). D-TMC-25 (context.md): sustituida su parte de la cuota. TIME-001: reglas formales, definición de "mostrado" y ejemplos. EDGE-002: causas del hueco "sin espacio". TQ-24 y TQ-25 cerradas. |
| 0.12 | 2026-10-28 | Orquestador, validada por Arquitecto y PO | Garantías de los snapshots (DS-US-TMC-018): D-TMC-40 (alcance de "un agente no los altera": Git y archivos del repo; escribir en la carpeta de datos no se impide; se detecta la alteración de un snapshot, no la desaparición entera de la carpeta) y D-TMC-41 (un snapshot no depende de nada del repo; dentro de la retención solo lo retira la purga por edad, tope o cuota, con aviso visto y 24 h). CONS-004: dos líneas de regla formal. TQ-26 abierta (packs abiertos en Windows, XP-49). |
| 0.13 | 2026-10-28 | Orquestador, validada por Arquitecto y PO | Overhead del snapshot previo (DS-US-TMC-020): D-TMC-42 (plazo por avance: 10 s sin avanzar y tope fijo de 100 s desde la aceptación; dos textos de `prior-timeout`) y D-TMC-43 (progreso del previo en la CLI y la TUI pasado 1 s; nunca en MCP ni con `--json`). CONS-001: nota del plazo. TQ-27 cerrada. Si el gate de 10 worktrees sigue en rojo, US-TMC-020 queda en parte y TS-TMC-002 entra en M2 con prioridad. |
