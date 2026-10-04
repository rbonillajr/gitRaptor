---
id: BR-MCP-001
title: "Reglas de Negocio — Servidor MCP"
type: business-rules
status: draft
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: mcp
related:
  context:
    - CTX-MCP-001
  stories:
    - US-GRD-016
tags:
  - mcp
  - servidor-mcp
  - herramientas-seguras
  - allowlist
  - seguridad
  - operacion-protegida
---

# Reglas de Negocio: Servidor MCP

> **Propósito**: reglas que gobiernan qué herramientas ofrece el Servidor MCP, sobre qué repo y worktree actúa, qué acepta, qué devuelve y cómo escribe. Los códigos llevan el prefijo `MCP` para no colisionar con los de otras features. Los IDs ajenos se califican: "BR-CONS-002 (Guardrails)", "Q39 de motor-local", "BR-CKP-ELIG-005", "BR-TMC-EDGE-004".

---

## Contexto

**Feature**: Servidor MCP (F-001-05)
**Enlace a contexto**: [context.md](./context.md) (CTX-MCP-001)
**Última actualización**: 2026-10-04

Principio que atraviesa todas las reglas: **el MCP no elige el repo, no autoriza y no escribe por su cuenta**. El ámbito sale del cwd del llamante, la decisión la toma Guardrails y la escritura la hace el ejecutor del daemon como operación protegida, con una operación del catálogo compartido con el Cockpit.

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos (VAL) | 6 | 5 |
| Cálculos de Negocio (CALC) | 5 | 2 |
| Elegibilidad (ELIG) | 6 | 4 |
| Workflows y Estados (WF) | 7 | 4 |
| Permisos y Autorizaciones (AUTH) | 5 | 5 |
| Consistencia de Datos (CONS) | 6 | 4 |
| Tiempo y Expiración (TIME) | 3 | 1 |
| Edge Cases (EDGE) | 10 | 6 |
| **Total** | **48** | **31** |

Criticidad "Alta" = crítica. Las cifras marcadas como tope (N) son el supuesto S-MCP-1: se fijan en la Dev Spec.

---

## 1. Validaciones de Datos

### BR-MCP-VAL-001: Rutas relativas al worktree

**Descripción**: toda ruta que recibe una herramienta (las rutas de `safe_commit`, la ruta opcional de `create_worktree`) es relativa y se valida antes de pedir nada al daemon. Se rechazan las rutas absolutas, las que salen del worktree o de la carpeta por defecto (`..`, enlaces que escapan), las UNC y las que llevan caracteres de control. Las rutas se tratan como literales: nunca como patrón ni como opción. El número de rutas por llamada tiene tope.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH ruta en la petición:
  IF ruta es absoluta OR contiene ".." que sale de la base OR es UNC OR tiene caracteres de control
     OR resuelve fuera de la base → rechazo "ruta no válida" + ruta (no confiable)
  ruta se pasa como pathspec literal
IF número de rutas > tope → rechazo "demasiadas rutas"
```

**Ejemplo**: `safe_commit` con `["src/app.rs", "../otro-repo/secreto"]` → rechazo de toda la llamada: "ruta fuera del worktree: ../otro-repo/secreto". Con `[":(glob)**"]` → se busca el archivo literal `:(glob)**`, que no existe → "ruta no encontrada".

**Fuentes**: Q-MCP-5, Q-MCP-7; SEC-02, SEC-11; BRD BR-16 (CVEs de `mcp-server-git`).

### BR-MCP-VAL-002: Nombres de rama y refs

**Descripción**: un nombre de rama que recibe una herramienta (rama nueva de `create_worktree`, filtro de `explain_history`) debe cumplir las reglas de nombres de Git, no empezar por `-` y respetar un tope de longitud. Nunca se acepta una expresión de revisión (`HEAD~3`, `@{-1}`, rangos).

**Criticidad**: Alta

**Regla formal**:
```
IF nombre no cumple las reglas de nombres de Git OR empieza por "-" OR longitud > tope
   OR contiene sintaxis de revisión → rechazo "nombre de rama no válido"
```

**Ejemplo**: `create_worktree` con rama `--upload-pack=evil` → "nombre de rama no válido". Con `feat..pagos` → igual. Con `feat/pagos` → continúa con BR-MCP-ELIG-004.

**Fuentes**: Q-MCP-7, Q-CKP-13; BRD BR-16; BR-CKP-VAL-001.

### BR-MCP-VAL-003: Mensaje de commit

**Descripción**: `safe_commit` exige un mensaje no vacío, con tope de longitud y sin caracteres de control (salvo saltos de línea). Si el repo tiene convenciones de commit configuradas (BR-11), el mensaje debe cumplirlas; si no las tiene, no se aplica ninguna. El mensaje nunca viaja como argumento de un comando y nunca se devuelve.

**Criticidad**: Alta

**Regla formal**:
```
IF mensaje vacío OR longitud > tope OR caracteres de control → rechazo + motivo
IF repo tiene convención de commit AND mensaje no la cumple → decisión de Guardrails (BR-MCP-AUTH-001)
mensaje se entrega al ejecutor fuera de argv
```

**Ejemplo**: repo con Conventional Commits y mensaje "arreglos" → denegado por Guardrails: "el mensaje no cumple la convención del repo: tipo(ámbito): descripción". Repo sin convención → "arreglos" se acepta.

**Fuentes**: Q-MCP-5; BRD BR-11; NFR-02.

### BR-MCP-VAL-004: Etiqueta de snapshot y nombre de agente

**Descripción**: la etiqueta de `snapshot` y el nombre que declara `register_agent` son textos cortos con tope de longitud y sin caracteres de control. El nombre de agente no puede ser un nombre reservado ni el de otro agente con sesión presente en el worktree. Ambos se guardan y se devuelven como texto no confiable.

**Criticidad**: Media

**Regla formal**:
```
IF longitud > tope OR caracteres de control → rechazo "texto no válido"
IF herramienta = register_agent AND (nombre reservado OR nombre de otro agente con sesión presente)
   → rechazo "nombre no permitido"
```

**Ejemplo**: `register_agent` con nombre "human" → "nombre no permitido". `snapshot` con etiqueta "antes de migrar" → aceptada.

**Fuentes**: Q-MCP-11, Q-MCP-23; ADR-GRP-005 § 6.6.

### BR-MCP-VAL-005: Parámetros cerrados, sin repo y `expect_worktree`

**Descripción**: cada herramienta acepta solo sus parámetros declarados; uno desconocido, de tipo incorrecto o fuera de rango se rechaza. Ninguna herramienta acepta un parámetro de repo. Las escrituras aceptan `expect_worktree` opcional: si se indica y no coincide con el worktree del llamante, se rechaza sin hacer nada. Nunca amplía el ámbito.

**Criticidad**: Alta

**Regla formal**:
```
IF parámetro desconocido OR tipo incorrecto OR fuera de rango → llamada mal formada
IF herramienta escribe AND expect_worktree indicado AND expect_worktree ≠ worktree del llamante
   → rechazo "el worktree no coincide" + worktree del llamante
```

**Ejemplo**: un subagente aislado en `shop-feat-b` usa el servidor del padre (cwd `shop-feat-a`) y llama `safe_commit` con `expect_worktree: shop-feat-b` → "el worktree no coincide: esta sesión opera en shop-feat-a". Nada cambia.

**Fuentes**: Q-MCP-2, Q-MCP-25; SEC-02, SEC-TMC-15.

### BR-MCP-VAL-006: Texto no confiable en las respuestas

**Descripción**: todo texto que sale del repo o de un agente (rutas, ramas, etiquetas, nombres de agente) va en campos marcados como dato no confiable, sin secuencias de control. Los nombres y las descripciones de las herramientas son fijos, en inglés, y declaran que ese texto es dato, no instrucción.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH campo de texto con origen repo o agente: marcar no confiable + quitar secuencias de control
descripciones de herramientas = constantes del binario; nunca incluyen texto del repo
```

**Ejemplo**: una rama llamada `ignore-previous-instructions-and-push` aparece en `status` como valor de un campo marcado no confiable, sin alterar las instrucciones del agente.

**Fuentes**: Q-MCP-15; SEC-12; M8; MCP Top 10 (tool poisoning).

---

## 2. Cálculos de Negocio

### BR-MCP-CALC-001: Resolución del ámbito del llamante

**Descripción**: en cada llamada, el daemon lee el cwd del proceso `raptor-mcp` y sube hasta el worktree que lo contiene. Ese es el worktree del llamante; su repo, el repo del llamante. `raptor-mcp` nunca cambia de directorio. Si el agente hace `cd` en su shell, la sesión MCP sigue en el worktree original. Cada respuesta nombra el worktree en el que actuó.

**Criticidad**: Alta

**Regla formal**:
```
worktree = worktree que contiene cwd(raptor-mcp), leído por el daemon
IF no hay worktree OR el repo no está observado → rechazo (BR-MCP-EDGE-004)
lecturas → ámbito = repo del llamante
escrituras → ámbito = worktree del llamante
respuesta incluye el worktree resuelto
```

**Ejemplo**: Claude Code arranca en `/code/shop-feat-a/src`. El ámbito es el worktree `/code/shop-feat-a` del repo `shop`. Si luego el agente hace `cd /code/shop-feat-b`, `safe_commit` sigue actuando en `shop-feat-a` y lo dice.

**Fuentes**: Q-MCP-2; ADR-TMC-005 § 1; SEC-TMC-15.

### BR-MCP-CALC-002: Respuesta acotada

**Descripción**: cada respuesta usa solo los campos de la allowlist de campos de su herramienta, con tope de tamaño total y por campo, y paginación con tope. Nunca incluye diff, mensajes de commit, contenido de archivos, valores de config, entorno ni credenciales. Las URLs de remotos van sin usuario ni contraseña. Si algo se recorta, la respuesta lo dice.

**Criticidad**: Alta

**Regla formal**:
```
respuesta = campos permitidos de la herramienta
IF tamaño > tope → recortar + "truncado" + cursor de la página siguiente
nunca: diff, mensaje, contenido, config, entorno, userinfo de URLs
```

**Ejemplo**: `status` en un worktree con 3.000 archivos modificados → las primeras N rutas, "3.000 en total, truncado" y un cursor.

**Fuentes**: Q-MCP-8 a Q-MCP-10, Q-MCP-16; Q-CKP-8; SEC-05, SEC-12; DEP-MCP-1.

### BR-MCP-CALC-003: Contenido de `status`

**Descripción**: `status` devuelve, del repo del llamante: los worktrees con sus sesiones y actor, rama, rutas modificadas (con tope), ahead/behind con su antigüedad, rama base confirmada, estado de protección y diagnósticos de Guardrails, estado del motor y huecos de observación, y el solicitante resuelto de la propia sesión.

**Criticidad**: Media

**Regla formal**:
```
status = {worktrees[sesiones, actor, rama, rutas modificadas, ahead/behind + antigüedad],
          base confirmada | no confirmada | pendiente, estado de protección + diagnósticos,
          estado del motor + huecos, solicitante = "agente X" | "sin atribuir", worktree del llamante}
```

**Ejemplo**: "Estás en shop-feat-a como claude-1. Base: main (confirmada). Protección: completa. 4 worktrees; shop-feat-b: claude-2, Activo, 2 por delante (hace 3 min)."

**Fuentes**: Q-MCP-10; Q-GRD-25; BR-WF-002 (Guardrails).

### BR-MCP-CALC-004: Contenido de `check_conflicts`

**Descripción**: `check_conflicts` devuelve la predicción que publica el motor: por par, el nivel (solape o conflicto previsto), las rutas y los rangos de líneas de los hunks sin contenido, su antigüedad, el estado "calculando" o "pendiente" y los límites declarados. Por defecto, solo los pares del worktree del llamante; con la opción de repo, todos. Nunca calcula nada propio ni devuelve el diff.

**Criticidad**: Media

**Regla formal**:
```
pares = opción repo ? todos : pares que incluyen al worktree del llamante
FOR EACH par: {lados, nivel, rutas, rangos de líneas, antigüedad, estado, límites}
```

**Ejemplo**: "conflicto previsto con shop-feat-b en src/pago.rs líneas 40-58, hace 12 s".

**Fuentes**: Q-MCP-8; Q-CKP-5, Q-CKP-6, Q-CKP-8; DEP-MCP-6.

### BR-MCP-CALC-005: Contenido de `explain_history`

**Descripción**: `explain_history` devuelve eventos del motor y del timeline de la Time Machine del repo del llamante: operación, actor, rama o worktree, oids, rutas tocadas (con tope), cuándo y nivel de cobertura del snapshot. Por defecto, los últimos 50 eventos; filtro por worktree o rama; paginación con tope. Sin mensajes de commit ni contenido.

**Criticidad**: Media

**Regla formal**:
```
eventos = timeline(repo del llamante) filtrado por worktree/rama, más recientes primero
página = min(solicitado, tope), por defecto 50
```

**Ejemplo**: "hace 5 min, claude-2 hizo commit a1b2c3d en feat-b (3 rutas); cobertura completa".

**Fuentes**: Q-MCP-9; S-MCP-2; DEP-MCP-6.

---

## 3. Reglas de Elegibilidad

### BR-MCP-ELIG-001: Matriz de precondiciones por herramienta

**Descripción**: cada herramienta se ejecuta solo si cumple sus precondiciones. Si no, devuelve un rechazo con motivo y la acción que lo resuelve. El ejecutor revalida al ejecutar y Guardrails decide después (BR-MCP-WF-001).

**Criticidad**: Alta

| Precondición | `status` | `check_conflicts` | `explain_history` | `register_agent` | `unregister_agent` | `safe_commit` | `safe_rebase` | `create_worktree` | `snapshot` | `undo` |
|---|---|---|---|---|---|---|---|---|---|---|
| Repo en la allowlist | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige |
| Solicitante atribuido | No | No | No | No | Exige (el propio) | Exige | Exige | Exige | Exige | Exige |
| `expect_worktree` coincide, si se indica | — | — | — | — | — | Exige | Exige | Exige | Exige | Exige |
| Base confirmada | No (estado declarado) | No ("pendiente") | No | No | No | No | Exige | Exige | No | No |
| Working tree limpio | — | — | — | — | — | — | Exige | — | — | — |
| Sin operación en curso | — | — | — | — | — | Exige | Exige | — | Exige | Exige |
| Sin otra sesión presente en el worktree | — | — | — | — | — | Sí; en compartido solo rutas explícitas | Exige | — | — | Solape → se detiene |
| Worktree disponible | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige | Exige |
| Decisión de Guardrails | — | — | — | — | — | Exige permitir | Exige permitir | Exige permitir | Exige permitir | Exige permitir |

**Regla formal**:
```
FOR EACH llamada:
  IF alguna precondición de la matriz falla → rechazo + motivo + acción
  ELSE continuar con BR-MCP-WF-001 (escrituras) o responder (lecturas)
```

**Ejemplo**: `safe_rebase` con base pendiente → "la rama base no está confirmada; el desarrollador la confirma desde GitRaptor". `status` en el mismo repo → responde con "base pendiente".

**Fuentes**: Q-MCP-2 a Q-MCP-11, Q-MCP-21, Q-MCP-22, Q-MCP-25.

### BR-MCP-ELIG-002: `safe_commit`

**Descripción**: commitea en el worktree del llamante las rutas indicadas o "todo lo preparado". En un worktree compartido solo acepta rutas explícitas. Nunca reescribe un commit, nunca salta los hooks y nunca crea un commit vacío. Respeta los hooks y la config del usuario. Devuelve el oid y las rutas, no el mensaje.

**Criticidad**: Alta

**Regla formal**:
```
IF worktree compartido AND modo = "todo lo preparado" → rechazo "indica las rutas: el worktree es compartido"
IF no hay cambios que commitear → rechazo "nada que commitear"
nunca: amend, no-verify, allow-empty
→ BR-MCP-WF-001; respuesta = {oid, rutas, worktree}
```

**Ejemplo**: claude-1 y claude-3 comparten `shop-feat-a`; claude-1 pide "todo lo preparado" → rechazo con la acción "indica tus rutas". Con `["src/a.rs"]` → commit `9f8e7d6` con esa ruta.

**Fuentes**: Q-MCP-5, Q-MCP-21, Q-MCP-27; BRD BR-11.

### BR-MCP-ELIG-003: `safe_rebase`

**Descripción**: rebasa la rama del worktree del llamante sobre la rama base confirmada. Exige working tree limpio, base confirmada, ninguna operación en curso y ninguna otra sesión presente en el worktree. No acepta otra rama ni otra base.

**Criticidad**: Alta

**Regla formal**:
```
IF base no confirmada OR worktree sucio OR operación en curso OR otra sesión presente
   → rechazo + motivo + acción
IF rama = rama base → decisión de Guardrails (BR-MCP-AUTH-001)
→ BR-MCP-WF-001 con la variante atómica (BR-MCP-WF-002)
```

**Ejemplo**: worktree con cambios sin commitear → "el worktree tiene cambios sin commitear; commitea o descártalos antes de rebasar".

**Fuentes**: Q-MCP-6, Q-MCP-21; Q-CKP-10.

### BR-MCP-ELIG-004: `create_worktree`

**Descripción**: crea una rama nueva desde la base confirmada y su worktree en el repo del llamante, con las reglas de BR-CKP-ELIG-005 y BR-CKP-VAL-001: ruta por defecto hermana del repo; ruta existente o no válida → error; nunca reutilizar. No registra ni lanza agente. La respuesta aclara que la sesión MCP sigue en el worktree original.

**Criticidad**: Media

**Regla formal**:
```
IF base no confirmada → rechazo + acción
IF rama existe OR ruta existe OR ruta no válida → rechazo + motivo
→ BR-MCP-WF-001; respuesta = {ruta creada, rama, "esta sesión sigue en <worktree del llamante>"}
```

**Ejemplo**: rama `feat/pagos` → worktree `/code/shop-feat-pagos` creado. "Para trabajar ahí, abre una sesión del agente en esa carpeta."

**Fuentes**: Q-MCP-7; Q-CKP-13.

### BR-MCP-ELIG-005: `snapshot` y `undo`

**Descripción**: `snapshot` toma un snapshot manual del worktree del llamante con una etiqueta corta, sujeto a una cuota y un rate limit propios. `undo` deshace la última operación propia del solicitante en su worktree (pila por worktree); si toca trabajo de otro actor, se detiene y lo informa. Un solicitante "sin atribuir" nunca hace `undo`. Redo y restaurar no existen por MCP.

**Criticidad**: Alta

**Regla formal**:
```
snapshot: IF cuota o rate limit superados → rechazo + acción
undo: op = última operación del worktree del llamante
  IF solicitante sin atribuir → rechazo (TQ-7)
  IF actor(op) ≠ solicitante → rechazo "no es tuya"
  IF solape con otro actor → se detiene + informa (D-TMC-13)
```

**Ejemplo**: claude-1 pide `undo` y la última operación del worktree es un commit de claude-3 → "la última operación es de claude-3; solo puedes deshacer lo tuyo".

**Fuentes**: Q-MCP-11; D-TMC-13, D-TMC-23, TQ-7, TQ-9; SEC-TMC-12.

### BR-MCP-ELIG-006: Herramientas de lectura

**Descripción**: `status`, `check_conflicts` y `explain_history` solo exigen que el repo esté en la allowlist y el worktree esté disponible. Las puede usar un solicitante "sin atribuir". No escriben ni pasan por Guardrails.

**Criticidad**: Media

**Regla formal**:
```
IF repo en la allowlist AND worktree disponible → responder (BR-MCP-CALC-002)
```

**Ejemplo**: un agente no detectado llama `status` → recibe el estado y "solicitante: sin atribuir; regístrate para escribir".

**Fuentes**: Q-MCP-3, Q-MCP-4.

---

## 4. Workflows y Estados

### BR-MCP-WF-001: Flujo de una herramienta de escritura

**Descripción**: toda escritura sigue el mismo orden: allowlist → ámbito → solicitante → `expect_worktree` → precondiciones → decisión de Guardrails con capa `mcp` → operación protegida (intención, snapshot previo, ejecución, registro) → respuesta. Un paso que falla corta el flujo con su motivo; nada cambia en el repo.

**Criticidad**: Alta

**Regla formal**:
```
allowlist → BR-MCP-EDGE-004 | ámbito → BR-MCP-CALC-001 | solicitante → BR-MCP-AUTH-002
→ expect_worktree → BR-MCP-VAL-005 | precondiciones → BR-MCP-ELIG-001
→ Guardrails(capa mcp, operación normalizada) → BR-MCP-AUTH-001
→ operación del catálogo como operación protegida → BR-MCP-CONS-002
→ respuesta {resultado, worktree, id de operación}
```

**Ejemplo**: `safe_commit` en un repo en la allowlist, por claude-1, mensaje válido → Guardrails permite → snapshot → commit → "commit 9f8e7d6 en shop-feat-a; deshacer con undo".

**Fuentes**: Q-MCP-1 a Q-MCP-5; ADR-GRD-003 § 4; D-TMC-10.

### BR-MCP-WF-002: Rebase atómico

**Descripción**: si el rebase de `safe_rebase` choca, se aborta solo dentro de la misma operación y el repo queda como antes. La respuesta lista las rutas en conflicto y el timeline registra "rebase abortado por conflicto". Las transiciones de refs del abort se registran en la operación y Guardrails no las reevalúa. Diverge de forma consciente del Cockpit (Q-CKP-11): el agente no tiene Abortar ni editor, y un estado a medias bloquea el undo y otras sesiones.

**Criticidad**: Alta

**Regla formal**:
```
rebase(rama del llamante, base confirmada) con flags fijados
IF termina → registro + respuesta {nuevo oid}
IF choca → abort en la misma operación
  IF abort ok → repo = estado previo; respuesta {conflicto, rutas}; evento "rebase abortado por conflicto"
  IF abort falla → BR-MCP-WF-003
```

**Ejemplo**: `safe_rebase` de `feat-a` sobre `main` choca en `src/pago.rs` → "rebase cancelado: conflicto en src/pago.rs; tu rama está como antes". En el timeline: "rebase abortado por conflicto (claude-1)".

**Fuentes**: Q-MCP-6; Q-CKP-11; DEP-MCP-2, DEP-MCP-5.

### BR-MCP-WF-003: Abort fallido

**Descripción**: si el abort automático falla, el worktree queda detenido como lo deja Git. Se informa al agente con motivo y acción (el desarrollador lo resuelve desde el Cockpit), el snapshot previo sigue disponible y toda escritura siguiente en ese worktree se rechaza por "operación en curso".

**Criticidad**: Alta

**Regla formal**:
```
IF abort falla → estado = "rebase en curso"; respuesta {error, acción: resolver desde el Cockpit}
siguientes escrituras en el worktree → rechazo por precondición (BR-MCP-ELIG-001)
```

**Ejemplo**: el abort falla porque un archivo está bloqueado → "no se pudo cancelar el rebase; pide al desarrollador que lo resuelva en raptor. Hay un snapshot previo".

**Fuentes**: Q-MCP-6; BR-TMC-EDGE-004.

### BR-MCP-WF-004: "Pedir confirmación" sin cola

**Descripción**: si Guardrails responde "pedir confirmación", mientras no exista la cola se trata como denegar con el motivo "requiere confirmación del desarrollador" y la acción (hacerlo desde el Cockpit). Cuando exista la cola, la herramienta devuelve "pendiente" con un id de petición y no bloquea esperando. Nunca se usa la elicitation de MCP como confirmación: la responde el cliente del agente.

**Criticidad**: Media

**Regla formal**:
```
IF decisión = pedir confirmación:
  IF cola no disponible → rechazo "requiere confirmación del desarrollador" + acción
  ELSE → respuesta {pendiente, id}; sin esperar
nunca elicitation como confirmación
```

**Ejemplo**: `safe_rebase` sobre una rama protegida con regla "confirmar" → "requiere confirmación del desarrollador; hazlo desde raptor".

**Fuentes**: Q-MCP-12; D-TMC-23; DEP-MCP-7.

### BR-MCP-WF-005: Registro y retiro de agente

**Descripción**: `register_agent` registra al solicitante en el worktree del llamante con un nombre validado; el origen queda "registrado" y es visible en `status` y en el timeline. Si el agente ya se detectaba ahí, confirma la misma sesión. `unregister_agent` solo retira el propio registro; retirar el de otro es reservado.

**Criticidad**: Media

**Regla formal**:
```
register_agent(nombre): validar (BR-MCP-VAL-004)
  IF sesión detectada del mismo agente → confirmar misma sesión (Q39 de motor-local)
  ELSE → nueva sesión origen "registrado"
unregister_agent: IF registro ≠ propio → rechazo "solo puedes retirar tu registro"
```

**Ejemplo**: Codex conectado a mano llama `register_agent("codex")` en `shop-feat-c` → sesión "codex (registrado)"; ya puede escribir.

**Fuentes**: Q-MCP-1, Q-MCP-4, Q-MCP-23; Q39 de motor-local; BR-02.

### BR-MCP-WF-006: Allowlist: opt-in, reservada y en cascada

**Descripción**: un repo entra en la allowlist solo cuando el desarrollador lo añade con un comando reservado; observarlo no basta. Solo se puede añadir un repo observado. Quitarlo también es reservado. Si el repo deja de estar observado, sale de la allowlist con aviso. Entrar o salir cambia el estado de protección.

**Criticidad**: Alta

**Regla formal**:
```
añadir(repo): comando reservado; IF repo no observado → rechazo "observa el repo primero"
quitar(repo): comando reservado
IF repo deja de estar observado → quitar de la allowlist + aviso
capa MCP activa ⇔ repo en la allowlist (BR-MCP-CONS-003)
```

**Ejemplo**: el desarrollador observa `shop` → el MCP responde "repo no habilitado" hasta que ejecuta el comando para habilitarlo. Al retirar `shop` de la observación: "shop también salió de la allowlist del MCP".

**Fuentes**: Q-MCP-3, Q-MCP-20; Q-GRD-15; DEP-MCP-3, DEP-MCP-4.

### BR-MCP-WF-007: Instalar y desinstalar

**Descripción**: `raptor mcp install` registra el servidor "gitraptor" en Claude Code con ámbito de usuario, usando la CLI `claude` por ruta absoluta y argv fijo. Antes muestra qué va a cambiar. Es idempotente, no toca otros servidores, nunca escribe en el repo y usa la ruta absoluta del binario instalado. Puede ofrecer añadir el repo actual a la allowlist, como comando reservado aparte. `raptor mcp uninstall` lo revierte. Para otros agentes responde "no soportado todavía".

**Criticidad**: Media

**Regla formal**:
```
IF agente ≠ claude-code → "no soportado todavía"
IF binario en caché de npx o temporal → rechazo "instala GitRaptor primero"
IF "gitraptor" ya registrado y es nuestro → sin cambios
IF "gitraptor" registrado y no es nuestro → BR-MCP-EDGE-009
IF claude no encontrado → imprimir el comando exacto
ELSE mostrar cambio → registrar con ámbito de usuario
uninstall → retirar solo "gitraptor"
```

**Ejemplo**: segunda ejecución de `raptor mcp install` → "gitraptor ya está instalado en Claude Code; nada que hacer". `--agent cursor` → "Cursor no está soportado todavía".

**Fuentes**: Q-MCP-14, Q-MCP-29; BRD BR-15, D2; SEC-14.

---

## 5. Permisos y Autorizaciones

### BR-MCP-AUTH-001: El MCP no autoriza; decide Guardrails

**Descripción**: cada escritura se traduce a su operación normalizada y Guardrails decide con capa `mcp` antes de pedir la operación. La decisión se registra una sola vez. La misma operación recibe la misma decisión que por los hooks.

**Criticidad**: Alta

**Regla formal**:
```
decisión = Guardrails(operación normalizada, actor, capa = mcp)
denegar → rechazo + regla + acción | pedir confirmación → BR-MCP-WF-004 | permitir → continuar
decisión(mcp) = decisión(hooks) para la misma operación
```

**Ejemplo**: `safe_rebase` en la rama base `main` → denegado: "regla: la rama base está protegida". Un `git rebase` crudo sobre `main` en el mismo repo recibe la misma denegación del hook.

**Fuentes**: ADR-GRD-003 § 4; BR-CONS-002 (Guardrails); US-GRD-016; DEP-MCP-5.

### BR-MCP-AUTH-002: Solicitante por ascendencia; "sin atribuir" solo lee

**Descripción**: el daemon atribuye cada llamada al agente que lanzó `raptor-mcp`, con canal `mcp`. Un solicitante "sin atribuir" solo puede usar las lecturas y `register_agent`; toda escritura se rechaza con la acción de registrarse.

**Criticidad**: Alta

**Regla formal**:
```
solicitante = resolver por ascendencia(raptor-mcp)
IF solicitante = sin atribuir AND herramienta escribe → rechazo "regístrate para escribir"
```

**Ejemplo**: un cliente MCP no detectado llama `snapshot` → "no se pudo identificar al agente; usa register_agent".

**Fuentes**: Q-MCP-4; ADR-TMC-005 § 1; TQ-7.

### BR-MCP-AUTH-003: Solo lo propio

**Descripción**: un agente escribe solo en su worktree, deshace solo sus operaciones y retira solo su registro. Nunca actúa sobre otro worktree ni sobre el trabajo de otro actor.

**Criticidad**: Alta

**Regla formal**:
```
escritura: ámbito = worktree del llamante
undo: actor(op) = solicitante
unregister: registro = solicitante
```

**Ejemplo**: claude-1 en `shop-feat-a` no tiene forma de pedir un commit en `shop-feat-b`: no hay parámetro para eso.

**Fuentes**: Q-MCP-2, Q-MCP-11, Q-MCP-23; D-TMC-23.

### BR-MCP-AUTH-004: Comandos reservados fuera del MCP

**Descripción**: ninguna herramienta permite editar la configuración, decidir en la cola, instalar o quitar hooks, usar la excepción consciente, añadir o quitar repos de la observación o de la allowlist, ni parar el daemon. El canal rechaza esos comandos si llegan desde la conexión del MCP.

**Criticidad**: Alta

**Regla formal**:
```
catálogo de herramientas ∩ comandos reservados = ∅
IF petición reservada llega por la conexión mcp → rechazo
```

**Ejemplo**: un agente intenta habilitar otro repo para el MCP → no existe herramienta; si fabrica la petición en el canal, se rechaza.

**Fuentes**: Q-MCP-3, Q-MCP-12; ADR-GRP-005 § 6, ADR-GRD-007 § 1; BR-AUTH-004 (Guardrails); Q40 de motor-local; SEC-03.

### BR-MCP-AUTH-005: Confused deputy

**Descripción**: todo proceso que descienda del ejecutor durante una operación (por ejemplo, un hook del usuario) se atribuye al solicitante de esa operación y no puede usar comandos reservados. Es requisito previo a cualquier herramienta de escritura.

**Criticidad**: Alta

**Regla formal**:
```
IF proceso desciende del ejecutor durante la operación O → solicitante(proceso) = solicitante(O)
IF ese proceso pide un comando reservado → rechazo
ninguna herramienta de escritura se entrega antes de cumplir esta regla
```

**Ejemplo**: el `pre-commit` del usuario, lanzado por un `safe_commit` de claude-1, pide un comando reservado (p. ej. relajar una regla) → se atribuye a claude-1 y se rechaza.

**Fuentes**: Q-MCP-5, Q-MCP-19; DEP-MCP-3.

---

## 6. Reglas de Consistencia de Datos

### BR-MCP-CONS-001: Cliente del daemon y catálogo compartido

**Descripción**: `raptor-mcp` es un cliente del daemon: no lee Git, no abre el perfil y no embebe el motor. Cada escritura es una operación del catálogo compartido con el Cockpit; el MCP no tiene operaciones propias fuera de él.

**Criticidad**: Alta

**Regla formal**:
```
lecturas = lo publicado por el daemon
escritura = operación del catálogo (DEP-CKP-7); IF no existe → no hay herramienta (DEP-MCP-2)
```

**Ejemplo**: `safe_commit` y el commit que en el futuro ofrezca el Cockpit son la misma operación del catálogo.

**Fuentes**: Q-MCP-1; ADR-GRP-005; ADR-TMC-002 § 5.

### BR-MCP-CONS-002: Toda escritura protegida y nunca remoto

**Descripción**: toda escritura del MCP lleva snapshot previo; sin snapshot no hay operación. Ninguna herramienta habla con el remoto.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH escritura: snapshot previo OK → ejecutar; ELSE rechazo, repo sin cambios
∄ herramienta con push, fetch o pull
```

**Ejemplo**: el disco está lleno y el snapshot no se puede tomar → "no se pudo tomar el snapshot previo; no se hizo el commit".

**Fuentes**: D-TMC-10; ADR-TMC-004; Q-MCP-1.

### BR-MCP-CONS-003: Allowlist ⊆ observados y capa MCP

**Descripción**: la allowlist vive en el perfil del usuario, nunca en el repo ni en la configuración del equipo, y siempre es un subconjunto de los repos observados. La capa MCP está activa si el repo está en la allowlist. Guardrails la consulta y nunca la escribe. Que ningún agente tenga el servidor instalado se muestra como diagnóstico "MCP no instalado", no como estado.

**Criticidad**: Alta

**Regla formal**:
```
allowlist ⊆ observados (siempre)
capa MCP activa ⇔ repo ∈ allowlist
estados de protección = los 4 de BR-WF-002 (Guardrails), sin cambio
```

**Ejemplo**: `shop` en la allowlist con hooks instalados → "completa". Sin servidor instalado → "completa · diagnóstico: MCP no instalado".

**Fuentes**: Q-MCP-3, Q-MCP-20, Q-MCP-26; ADR-GRD-005 § 2; Q-GRD-15.

### BR-MCP-CONS-004: Catálogo de herramientas fijo

**Descripción**: el servidor ofrece solo la capability `tools`, por stdio, con la lista fija de diez herramientas. Los nombres y las descripciones son constantes del binario y no cambian durante la sesión. Las anotaciones de solo lectura o destructiva son orientativas, no control.

**Criticidad**: Alta

**Regla formal**:
```
capabilities = {tools}; sin sampling, prompts, resources ni listChanged
herramientas = lista fija; nunca cambia en caliente
```

**Ejemplo**: un repo con un archivo que intenta redefinir la descripción de `safe_commit` no cambia nada: la descripción no sale del repo.

**Fuentes**: Q-MCP-13, Q-MCP-15; MCP Top 10 (tool poisoning, rug pull).

### BR-MCP-CONS-005: Errores estables con motivo y acción

**Descripción**: un rechazo de dominio es un resultado de la herramienta marcado como error, con código estable, mensaje de plantilla fija y parámetros marcados como no confiables. Siempre dice motivo y acción. Los errores de protocolo se reservan para llamadas mal formadas. Nunca hay trazas ni rutas fuera del repo. Mensajes en inglés o español según el locale del usuario.

**Criticidad**: Media

**Regla formal**:
```
rechazo de dominio → resultado {isError, código, plantilla(código, params no confiables), acción}
llamada mal formada → error de protocolo
```

**Ejemplo**: `MCP_REPO_NOT_ALLOWED`: "Este repo no está habilitado para el MCP. El desarrollador lo habilita con el comando de allowlist." (el comando exacto lo fija la Dev Spec).

**Fuentes**: Q-MCP-17; ADR-GRD-003 (M-05); NFR-10.

### BR-MCP-CONS-006: Registro y medición

**Descripción**: cada decisión de Guardrails tomada para el MCP se registra una sola vez con capa `mcp`. Las operaciones del MCP quedan en el timeline con su actor y canal. Los KPIs del MCP miden solo sus herramientas; lo que el agente hace con Git crudo lo cubren hooks y observación.

**Criticidad**: Media

**Regla formal**:
```
decisión(mcp) registrada 1 vez
KPIs MCP = {denegadas ejecutadas = 0, escrituras sin snapshot = 0, datos fuera de allowlist = 0,
            escrituras fuera del worktree = 0, reservados desde descendientes = 0}
bloqueos contados por capa; undos por MCP cuentan en el KPI de undos
```

**Ejemplo**: el informe mensual muestra "acciones bloqueadas: 12 por mcp, 7 por hooks".

**Fuentes**: Q-MCP-18, Q-MCP-24; ADR-GRD-003 § 4; BRD § 9.

---

## 7. Reglas de Tiempo y Expiración

### BR-MCP-TIME-001: Tiempo por llamada y rate limit

**Descripción**: cada llamada tiene un tiempo máximo; cada conexión, un rate limit; los snapshots manuales, una cuota y un rate limit propios. Una escritura que tarda por el snapshot declara su estado en la respuesta.

**Criticidad**: Media

**Regla formal**:
```
IF llamadas por conexión > límite → rechazo "demasiadas llamadas; espera N s"
IF tiempo > máximo AND lectura → error con acción
IF escritura en curso al vencer el tiempo → respuesta con estado + id de operación
```

**Ejemplo**: un agente en bucle pide 200 snapshots en un minuto → los que pasan del límite se rechazan con el tiempo de espera.

**Fuentes**: Q-MCP-11, Q-MCP-16; SEC-08, SEC-TMC-12; S-MCP-1.

### BR-MCP-TIME-002: Cancelación y desconexión

**Descripción**: si el cliente cancela la llamada o se desconecta, una escritura ya iniciada termina igualmente y queda en el timeline. Una lectura se abandona.

**Criticidad**: Alta

**Regla formal**:
```
IF cancelación OR desconexión durante escritura → la operación termina + registro
IF durante lectura → abandonar
```

**Ejemplo**: el usuario interrumpe a Claude Code durante un `safe_rebase` → el rebase termina (o se aborta, BR-MCP-WF-002) y aparece en `explain_history`.

**Fuentes**: Q-MCP-16; NFR-01.

### BR-MCP-TIME-003: Arranque del daemon bajo demanda

**Descripción**: el daemon se arranca con la primera llamada a una herramienta, no al iniciar el servidor, con entorno limpio.

**Criticidad**: Baja

**Regla formal**:
```
al iniciar raptor-mcp → no arrancar el daemon
primera llamada a herramienta AND daemon no corre → arrancarlo (entorno limpio) → BR-MCP-EDGE-001 si falla
```

**Ejemplo**: Claude Code abre una sesión sin usar herramientas de GitRaptor → el daemon no se arranca.

**Fuentes**: Q-MCP-16, Q-MCP-22; ADR-GRP-005; SEC-10.

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-MCP-EDGE-001: Daemon caído o no arrancable

**Descripción**: si el daemon no corre, `raptor-mcp` lo arranca. Si no puede, la herramienta devuelve un error con la acción para el desarrollador. Nunca embebe el motor ni lee Git por su cuenta.

**Criticidad**: Media

**Ejemplo**: el binario del daemon no está en su ruta → "GitRaptor no está en marcha y no se pudo arrancar; revisa la instalación de GitRaptor".

**Fuentes**: Q-MCP-22; ADR-GRP-005.

### BR-MCP-EDGE-002: Rama base no confirmada o pendiente

**Descripción**: las lecturas responden con el estado declarado de la base. Las escrituras que la necesitan (`safe_rebase`, `create_worktree`) se rechazan con la acción para confirmarla. El resto sigue y aplica el conjunto mínimo de Guardrails.

**Criticidad**: Alta

**Ejemplo**: base pendiente → `safe_commit` funciona; `safe_rebase` → "la rama base está pendiente de confirmar".

**Fuentes**: Q-MCP-22; Q-GRD-5, Q-GRD-23; Q-CKP-27.

### BR-MCP-EDGE-003: Worktree compartido

**Descripción**: con más de una sesión en el worktree: `safe_commit` solo acepta rutas explícitas; `safe_rebase` se bloquea si hay otra sesión presente; `undo` se detiene ante solape con otro actor.

**Criticidad**: Alta

**Ejemplo**: claude-1 y claude-3 en `shop-feat-a` → `safe_rebase` de claude-1: "claude-3 sigue trabajando en este worktree".

**Fuentes**: Q-MCP-21; Q7 de motor-local; D-TMC-13.

### BR-MCP-EDGE-004: Fuera de la allowlist o fuera de un worktree observado

**Descripción**: si el repo del llamante no está en la allowlist, todas las herramientas, también las lecturas, responden "repo no habilitado para el MCP" con la acción y sin ningún dato del repo. Si el cwd no está en un worktree de un repo observado, se rechaza igual.

**Criticidad**: Alta

**Ejemplo**: Claude Code arrancado en `~/descargas` → "esta carpeta no está en un repo observado por GitRaptor". En `shop` sin habilitar → "repo no habilitado para el MCP; el desarrollador lo habilita con el comando de allowlist".

**Fuentes**: Q-MCP-2, Q-MCP-3; SEC-TMC-15.

### BR-MCP-EDGE-005: Repo o worktree no disponible

**Descripción**: un repo o worktree que el motor marca "no disponible" no se opera, aunque esté en la allowlist. Eso incluye uno borrado o desmontado, uno de otro propietario en el SO (mismo criterio que `safe.directory`) y un worktree que incumple SEC-11: `gitdir` no bidireccional, o raíz en `/`, en `$HOME` o en un ancestro del repo. La comprobación se hace en cada llamada, porque la propiedad puede cambiar. Primero se comprueba "no habilitado" (allowlist) y después "no disponible". Ninguna de las dos respuestas lleva datos del repo. La acción se indica solo como texto: nunca se añade el repo a `safe.directory` ni se escribe en la configuración global (Q17 de motor-local).

**Criticidad**: Alta

**Regla formal**:
```
IF repo ∉ allowlist → "repo no habilitado para el MCP" + acción          (sin datos)
ELSE IF repo o worktree no disponible (borrado, otro propietario, SEC-11) → "no disponible" + acción en texto   (sin datos)
evaluado en cada llamada
```

**Ejemplo**: el worktree se borró a mano mientras la sesión seguía → "el worktree shop-feat-a ya no existe". Si el repo pasa a ser de otro usuario → "repo no disponible: pertenece a otro usuario", sin rutas ni ramas.

**Fuentes**: Q-MCP-31; BR-EDGE-001 (motor-local); SEC-11; ADR-GRP-009.

### BR-MCP-EDGE-006: Subagente en otro worktree

**Descripción**: un subagente aislado en otro worktree que comparte el servidor del padre opera en el ámbito del padre. Si indica `expect_worktree` con su propio worktree, sus escrituras se rechazan en lugar de caer en el worktree del padre.

**Criticidad**: Alta

**Ejemplo**: ver BR-MCP-VAL-005.

**Fuentes**: Q-MCP-25; R-MCP-2.

### BR-MCP-EDGE-007: Rebase de una rama ya empujada

**Descripción**: el MCP no deniega el rebase de una rama con upstream: lo decide Guardrails. Si se permite, la respuesta avisa de que la rama diverge de su upstream. El force-push posterior lo deniega el conjunto mínimo seguro.

**Criticidad**: Media

**Ejemplo**: `feat-a` ya empujada → rebase hecho: "tu rama ahora diverge de origin/feat-a; el force-push está bloqueado por política".

**Fuentes**: Q-MCP-28; Q-GRD-5.

### BR-MCP-EDGE-008: Operación en curso o HEAD separado

**Descripción**: con un merge o rebase a medias en el worktree, las escrituras se rechazan por precondición hasta que se resuelva. Con HEAD separado, `safe_commit` y `safe_rebase` se rechazan con la acción "crea una rama o cámbiate a una". `snapshot` y `undo` siguen permitidos. Las comprobaciones son precondiciones del ejecutor del daemon, dentro del catálogo compartido. El ejecutor las vuelve a comprobar justo antes de ejecutar. Primero se comprueba la operación en curso, porque un rebase a medias también deja HEAD separado.

**Criticidad**: Alta

**Regla formal**:
```
IF operación de Git en curso → rechazo "operación en curso" + acción        (primero)
ELSE IF HEAD separado AND herramienta ∈ {safe_commit, safe_rebase} → rechazo + "crea una rama o cámbiate a una"
ELSE continuar                                                                (snapshot, undo: permitidos)
revalidar justo antes de ejecutar (ejecutor, DEP-MCP-2)
```

**Ejemplo**: el agente dejó un `git rebase` crudo a medias → `safe_commit` responde "hay un rebase en curso en este worktree". Con `git checkout <oid>` → `safe_commit` responde "HEAD separado: crea una rama o cámbiate a una". El trabajo no se perdería (el reflog y la Time Machine lo conservan), pero la regla evita un estado confuso para el agente.

**Fuentes**: Q-MCP-6, Q-MCP-30; BR-TMC-EDGE-004; BR-CKP-EDGE-002; DEP-MCP-2.

### BR-MCP-EDGE-009: Instalación en conflicto o agente no soportado

**Descripción**: si en Claude Code ya existe un servidor "gitraptor" que no es el nuestro, la instalación no lo sobrescribe: error con la acción. Para Cursor, Codex o Copilot, "no soportado todavía".

**Criticidad**: Media

**Ejemplo**: el usuario tenía un "gitraptor" apuntando a otro binario → "ya existe un servidor gitraptor que no es de este GitRaptor; retíralo con claude mcp remove gitraptor y vuelve a instalar".

**Fuentes**: Q-MCP-14, Q-MCP-29; D2.

### BR-MCP-EDGE-010: Archivos sin seguimiento e ignorados en `safe_commit`

**Descripción**: un archivo sin seguimiento entra en el commit solo si se nombra en las rutas explícitas. "Todo lo preparado" no añade archivos sin seguimiento. Un archivo ignorado nunca se añade, aunque se nombre.

**Criticidad**: Media

**Ejemplo**: `safe_commit` con `[".env"]` ignorado → "la ruta .env está ignorada por Git; no se añade".

**Fuentes**: Q-MCP-27.

---

## Matriz de Priorización

Alineada con el orden de entrega Q-MCP-19: 1) canal + allowlist + `status` + install + register/unregister; 2) confused deputy; 3) snapshot/undo + `safe_commit`; 4) `check_conflicts` y `explain_history`; 5) `safe_rebase` y `create_worktree`.

| Regla | Criticidad | Prioridad |
|-------|------------|-----------|
| CALC-001, CALC-002, CALC-003, VAL-004, VAL-005, VAL-006, ELIG-001, ELIG-006, WF-005, WF-006, WF-007, AUTH-002, AUTH-004, CONS-001, CONS-003, CONS-004, CONS-005, TIME-001, TIME-003, EDGE-001, EDGE-004, EDGE-005, EDGE-009 | Alta/Media/Baja | 🔴 P0 (entrega 1: canal, allowlist, `status`, install, registro) |
| AUTH-005 | Alta | 🔴 P0 (entrega 2: requisito previo a toda escritura) |
| VAL-001, VAL-003, ELIG-002, ELIG-005, WF-001, WF-004, AUTH-001, AUTH-003, CONS-002, CONS-006, TIME-002, EDGE-002, EDGE-003, EDGE-008, EDGE-010 | Alta/Media | 🟡 P1 (entrega 3: snapshot/undo, `safe_commit`) |
| CALC-004, CALC-005 | Media | 🟡 P1 (entrega 4: depende de DEP-MCP-6) |
| VAL-002, ELIG-003, ELIG-004, WF-002, WF-003, EDGE-006, EDGE-007 | Alta/Media | 🟢 P2 (entrega 5: depende de ADR-CKP-002) |

Los IDs omiten el prefijo `BR-MCP-`. WF-004 entrega hoy la variante "denegar"; la variante "pendiente" queda bloqueada por DEP-MCP-7.

---

## Trazabilidad

### Reglas → Capacidades del BRD

| Capacidad | Reglas |
|-----------|--------|
| BR-14 Herramientas de alto nivel y seguras | CALC-001 a CALC-005, ELIG-001 a ELIG-006, WF-001 a WF-005, AUTH-001 a AUTH-003, CONS-001, CONS-002, TIME-002, TIME-003, EDGE-001 a EDGE-003, EDGE-005 a EDGE-008, EDGE-010 |
| BR-15 Instalación en un paso (parcial por D2: solo Claude Code) | WF-007, EDGE-009 |
| BR-16 Endurecimiento de seguridad | VAL-001 a VAL-006, CALC-002, WF-006, AUTH-004, AUTH-005, CONS-003 a CONS-005, TIME-001, EDGE-004 |
| NFR-02 Sin shell, entradas validadas, solo allowlist, revisión por release | VAL-001 a VAL-006, WF-006, AUTH-004, AUTH-005, CONS-003, CONS-006 |

### Reglas → User Stories

Pendiente: las historias se generan tras la aprobación del requerimiento (ADR-018).

**Historia de otra feature que depende de estas reglas**: [US-GRD-016](../guardrails/user-stories/US-GRD-016-misma-decision-por-mcp.md) (Guardrails), bloqueada por esta feature. La desbloquean BR-MCP-WF-001, BR-MCP-AUTH-001, BR-MCP-WF-006 y BR-MCP-CONS-003; el desbloqueo real llega con ADR-MCP-001 y la implementación. Su estado no cambia en este documento.

### Reglas → Criterios de Aceptación

Cada regla tendrá al menos un escenario Gherkin, incluido uno negativo, en su historia. Las reglas VAL y AUTH-005 se verifican además con el corpus de seguridad (traversal, refs maliciosas, UNC, inyección de argumentos, confused deputy). La demo del BRD § 13 es prueba de aceptación del feature en dos partes (Q-MCP-18).

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 0.1 | 2026-10-04 | PO (AADD) | Versión inicial: 48 reglas a partir de Q-MCP-1 a Q-MCP-31 (decisión del orquestador, validada por PO y Arquitecto) y de las decisiones heredadas de motor-local, Cockpit, Time Machine y Guardrails. |
