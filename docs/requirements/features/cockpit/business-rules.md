---
id: BR-CKP-001
title: "Reglas de Negocio — Cockpit"
type: business-rules
status: draft
created: 2026-10-04
updated: 2026-10-04
domain: GRP
epic: E-001
feature: cockpit
related:
  context:
    - CTX-CKP-001
tags:
  - cockpit
  - tui
  - prediccion-conflictos
  - acciones-por-agente
  - operacion-protegida
---

# Reglas de Negocio: Cockpit

> **Propósito**: reglas que gobiernan qué muestra el Cockpit, cuándo ofrece cada acción y cómo la ejecuta. Los códigos llevan el prefijo `CKP` para no colisionar con los de otras features. Los IDs ajenos se califican: "BR-WF-001 (motor-local)", "BR-WF-001 (Guardrails)", "BR-TMC-EDGE-004".

---

## Contexto

**Feature**: Cockpit (F-001-02)
**Enlace a contexto**: [context.md](./context.md) (CTX-CKP-001)
**Última actualización**: 2026-10-04

Principio que atraviesa todas las reglas: **el Cockpit presenta, no calcula ni autoriza**. El estado lo publica el motor, la decisión la toma Guardrails y la escritura la hace el ejecutor del daemon como operación protegida.

---

## Categorías de Reglas

| Categoría | Cantidad | Críticas |
|-----------|----------|----------|
| Validaciones de Datos (VAL) | 3 | 2 |
| Cálculos de Negocio (CALC) | 5 | 3 |
| Elegibilidad (ELIG) | 6 | 4 |
| Workflows y Estados (WF) | 7 | 5 |
| Permisos y Autorizaciones (AUTH) | 4 | 4 |
| Consistencia de Datos (CONS) | 7 | 4 |
| Tiempo y Expiración (TIME) | 4 | 1 |
| Edge Cases (EDGE) | 9 | 3 |
| **Total** | **45** | **26** |

Criticidad "Alta" = crítica.

---

## 1. Validaciones de Datos

### BR-CKP-VAL-001: Parámetros de crear worktree

**Descripción**: crear un worktree pide un nombre de rama nueva y acepta una ruta. La rama debe ser válida según las reglas de nombres de Git y no existir. La ruta por defecto es hermana del repo (`<padre>/<repo>-<rama-saneada>`) y es configurable en el perfil (p. ej. para el esquema de Orca). Una ruta que ya existe o que no es válida (incluidas las rutas UNC, SEC-02) se rechaza con un error accionable. Nunca se reutiliza ni se sobrescribe un directorio.

**Criticidad**: Alta

**Regla formal**:
```
IF rama no cumple las reglas de nombres de Git OR rama ya existe → rechazo + motivo
ruta = ruta indicada ?? plantilla del perfil ?? <padre>/<repo>-<rama-saneada>
IF ruta existe OR ruta no válida (UNC, SEC-02) → rechazo + motivo + acción
ELSE → continuar con BR-CKP-ELIG-005
```

**Ejemplo**: rama `feat/pagos` en `/code/shop` → ruta propuesta `/code/shop-feat-pagos`. Si ya existe, error "la ruta ya existe; elige otra" y nada cambia. Rama `feat..pagos` → "nombre de rama no válido".

**Fuentes**: Q-CKP-13; SEC-02.

### BR-CKP-VAL-002: Texto no confiable saneado antes de pintarlo

**Descripción**: todo texto que viene del repo o de un agente (rutas, ramas, mensajes de commit, nombres de agente declarados, diagnósticos) se sanea antes de mostrarlo. Las secuencias de control no llegan a la terminal.

**Criticidad**: Alta

**Regla formal**:
```
FOR EACH texto de origen repo o agente:
  mostrar = sanear(texto)          (secuencias de control neutralizadas, SEC-12)
```

**Ejemplo**: una rama cuyo nombre lleva una secuencia de escape que borraría la pantalla se muestra con la secuencia neutralizada y visible; la TUI no cambia de estado.

**Fuentes**: SEC-12; DEP-CKP-9.

### BR-CKP-VAL-003: El valor del editor se interpreta sin shell

**Descripción**: el valor del editor (configuración, o `$VISUAL`/`$EDITOR`) se separa en palabras sin shell ni expansiones. Un valor con metacaracteres de shell se rechaza con su motivo. Es una diferencia declarada con Git, que sí usa shell. La clave del editor solo se admite en el perfil y en la configuración local personal, nunca en la del equipo.

**Criticidad**: Media

**Regla formal**:
```
editor = config (perfil | local personal) ?? $VISUAL ?? $EDITOR      (equipo: no admitido)
IF editor contiene metacaracteres de shell → rechazo + motivo
ELSE argv = separar_en_palabras(editor) + [ruta]
```

**Ejemplo**: `code --wait` → se lanza con dos palabras más la ruta. `vim; rm -rf ~` → rechazado: "el editor contiene caracteres de shell no admitidos".

**Fuentes**: Q-CKP-9; Q24 de motor-local; DEP-CKP-13.

---

## 2. Cálculos de Negocio

### BR-CKP-CALC-001: Se presenta lo publicado, sin cálculo propio

**Descripción**: cada fila muestra rama, estado de sesión, archivos modificados, ahead/behind y última actividad tal como los publica el motor. El ahead/behind se presenta "según la copia local del remoto", con su antigüedad, porque el motor no consulta el remoto. Si el motor no publica un campo, el Cockpit no lo deriva: lo muestra como no disponible y queda la dependencia.

**Criticidad**: Alta

**Regla formal**:
```
valor mostrado = valor publicado por el motor
ahead/behind → etiqueta "según copia local del remoto" + antigüedad        (Q12 de motor-local)
IF campo no publicado → "no disponible"; nunca se calcula en la TUI
```

**Ejemplo**: `feat-pagos` muestra "↑3 ↓1 según copia local (hace 2 h)". La última actividad no se publica aún → "no disponible" hasta DEP-CKP-4.

**Fuentes**: Q-CKP-29; Q12 de motor-local; ADR-GRP-005, ADR-GRP-013; DEP-CKP-4.

### BR-CKP-CALC-002: Predicción en dos niveles con límites declarados

**Descripción**: el motor publica dos niveles por par. **Solape** (⚠): los dos lados modifican los mismos archivos, incluido lo sin commitear. **Conflicto previsto** (⚡): el merge en seco de lo commiteado detecta conflicto; se acompaña de archivos y hunks. Los pares son cada worktree contra la base confirmada y cada par de worktrees con trabajo propio. La vista declara siempre sus límites: el solape incluye lo sin commitear, el conflicto previsto solo lo commiteado, y los drivers de merge y `.gitattributes` del usuario no se aplican. La predicción nunca escribe en el repo del usuario.

**Criticidad**: Alta

**Regla formal**:
```
pares = {(worktree, base confirmada)} ∪ {(w1, w2) | w1, w2 con trabajo propio}
FOR EACH par:
  solape            = archivos modificados en común (commiteado + sin commitear)     → ⚠
  conflicto previsto = merge en seco de lo commiteado detecta conflicto              → ⚡ + archivos + hunks
IF base no confirmada o pendiente → pares contra la base = "pendiente"               (BR-CKP-WF-005)
declaración de límites visible junto a la predicción
Constraint: la predicción no deja rastro en el repo del usuario                       (NFR-01)
```

**Ejemplo (demo BRD § 13)**: claude-1 y claude-2 editan `src/api.rs` en la misma función y commitean → ⚡ "claude-1 ↔ claude-2: src/api.rs, hunk 42-58". claude-3 edita sin commitear `README.md`, que también tocó claude-4 → solo ⚠.

**Fuentes**: Q-CKP-5, Q-CKP-25; NFR-01; ADR-GRP-009; DEP-CKP-1.

### BR-CKP-CALC-003: Frescura y antigüedad de la predicción

**Descripción**: la predicción está fuera del gate de 500 ms. Tras un commit se recalculan solo los pares de ese worktree; el objetivo es ≤ 5 s p95 con 10 worktrees (supuesto S-CKP-1). En el cálculo inicial y tras mover la base se muestra "calculando". Siempre se ve la antigüedad del resultado y nunca se presenta un resultado viejo como actual.

**Criticidad**: Alta

**Regla formal**:
```
evento commit en W → recalcular pares que incluyen W                    (objetivo ≤ 5 s p95, S-CKP-1)
cálculo inicial OR base movida → estado "calculando"
resultado mostrado = último resultado + antigüedad
IF hay recálculo en curso → resultado marcado "desactualizado, recalculando"
```

**Ejemplo**: claude-2 commitea; durante 3 s el par claude-1 ↔ claude-2 aparece "hace 40 s · recalculando" y luego "hace 0 s".

**Fuentes**: Q-CKP-6; S-CKP-1.

### BR-CKP-CALC-004: Grafo acotado sobre la base confirmada

**Descripción**: un carril por rama de worktree, desde su merge-base con la base confirmada. Cada carril muestra una ventana acotada (≈ 50 commits, el resto colapsado; la cifra la afina diseño). El color es por carril (agente de la sesión del worktree), no por commit. El actor por commit solo se muestra si el motor publica la relación commit→evento; si no, "sin atribuir". No es un cliente Git completo.

**Criticidad**: Media

**Regla formal**:
```
carriles = {rama de cada worktree} sobre base confirmada
commits del carril = merge-base..rama, últimos ≈ 50; resto colapsado        (S-CKP-2)
color = agente de la sesión del worktree (por carril)
actor por commit = publicado ? agente : "sin atribuir"
IF base no calculable → sin carriles + motivo                               (BR-CKP-EDGE-001)
```

**Ejemplo**: `feat-pagos` con 72 commits desde el merge-base → 50 visibles y "22 más" colapsados, todos en el color de claude-1.

**Fuentes**: Q-CKP-4; S-CKP-2; DEP-CKP-2.

### BR-CKP-CALC-005: Contenido de "ver diff"

**Descripción**: el diff muestra lo que entraría al merge (rama frente a su merge-base con la base confirmada) y, aparte, lo sin commitear. Se pide al daemon bajo demanda, calculado sin filtros, con topes por archivo y en total y con detección de binarios. Nunca se expone por el MCP. En un worktree compartido no se atribuye por archivo.

**Criticidad**: Media

**Regla formal**:
```
diff = { commiteado: merge-base(base confirmada, rama)..rama ; sin commitear: working tree }
IF archivo binario → "binario" sin contenido
IF supera tope por archivo o total → truncado + aviso
IF worktree compartido → sin atribución por archivo                       (Q7 de motor-local)
Constraint: nunca por el MCP
```

**Ejemplo**: `feat-pagos` con 3 commits y 1 archivo sin commitear → dos secciones: "entraría al merge (3 archivos)" y "sin commitear (1 archivo)". `logo.png` aparece como "binario".

**Fuentes**: Q-CKP-8; Q7 de motor-local; DEP-CKP-3.

---

## 3. Reglas de Elegibilidad

### BR-CKP-ELIG-001: Matriz de precondiciones por acción

**Descripción**: cada acción se ofrece solo si cumple sus precondiciones. Si no, se muestra desactivada con el motivo y, si existe, la acción que la desbloquea. Las precondiciones son de presentación: el ejecutor las revalida al ejecutar (BR-CKP-CONS-004) y Guardrails decide después (BR-CKP-AUTH-001).

**Criticidad**: Alta

| Precondición | Ver diff | Abrir editor | Merge | Rebase | Descartar | Crear worktree |
|---|---|---|---|---|---|---|
| Sesión Activa en el worktree del agente | Sí | Sí | Sí, con aviso y confirmación "hasta el commit X" | No | No | — |
| Sesión Inactiva en el worktree del agente | Sí | Sí | Sí | No | No | — |
| Sesión Terminado o sin agente | Sí | Sí | Sí | Sí | Sí | — |
| Base confirmada | No exige | No exige | Exige | Exige | No exige | Exige |
| Working tree limpio | No exige | No exige | Exige en el destino | Exige en el del agente | Confirmación si hay trabajo sin integrar | — |
| Sin operación en curso | No exige | No exige | En ambos worktrees | En el del agente | Exige | — |
| Destino sin sesión presente | — | — | Exige | — | — | — |
| Worktree principal | Sí | Sí | No (es el origen) | No | No | — |
| Rama base o rama protegida | Sí | Sí | Guardrails decide | Guardrails decide | No | — |
| Worktree no disponible | No | No | No | No | No | — |
| ⚡ conflicto previsto en el par | Sí | Sí | Aviso y confirmación | Aviso y confirmación | — | — |

**Regla formal**:
```
FOR EACH acción en la fila seleccionada:
  IF alguna precondición de la matriz falla → acción desactivada + motivo + acción que la desbloquea
  ELSE acción ofrecida
```

**Ejemplo**: worktree de claude-2 con sesión Inactiva → "Rebase" desactivado: "el agente sigue en este directorio; espera a que termine". Base pendiente → "Merge" desactivado: "confirma la rama base" con la acción para confirmarla.

**Fuentes**: Q-CKP-8 a Q-CKP-13, Q-CKP-27.

### BR-CKP-ELIG-002: Aprobar y hacer merge

**Descripción**: integra la rama del agente en la base confirmada, solo en local, en el worktree que tiene la base sacada (es único). Exige base confirmada, worktree destino limpio y sin sesión presente, y ninguna operación en curso en los dos worktrees. Una sesión Activa en el worktree del agente no bloquea: se avisa y se confirma hasta qué commit se integra. Con ⚡ en el par se avisa y se pide confirmación antes. Si la base es protegida, decide Guardrails (BR-CKP-AUTH-001).

**Criticidad**: Alta

**Regla formal**:
```
REQUIRE base confirmada                          ELSE bloquear + ofrecer confirmarla (acción reservada)
destino = worktree con la base sacada            IF ninguno → BR-CKP-EDGE-009
REQUIRE destino limpio AND destino sin sesión presente
REQUIRE sin operación en curso en destino ni en worktree del agente
IF sesión Activa en worktree del agente → aviso + confirmación "se integra hasta el commit X"
IF ⚡ en el par (rama, base) → aviso + confirmación
→ BR-CKP-WF-002
```

**Ejemplo**: claude-1 sigue Activo en `feat-pagos` con HEAD `a1b2c3` → "el agente sigue activo; se integra hasta el commit a1b2c3. ¿Continuar?" Si sí, el merge se hace en el worktree principal, que tiene `main` sacada.

**Fuentes**: Q-CKP-10, Q-CKP-26.

### BR-CKP-ELIG-003: Rebase de la rama del agente

**Descripción**: rebasa la rama del agente sobre la base confirmada, en el worktree del agente. Se bloquea con cualquier sesión presente (Activa o Inactiva), porque el proceso del agente sigue vivo en ese directorio. Exige working tree limpio, base confirmada y ninguna operación en curso. Con ⚡ se avisa y se confirma.

**Criticidad**: Alta

**Regla formal**:
```
REQUIRE sin sesión presente en el worktree del agente
REQUIRE working tree limpio AND base confirmada AND sin operación en curso
IF ⚡ en el par (rama, base) → aviso + confirmación
→ BR-CKP-WF-002
```

**Ejemplo**: claude-3 Terminado en `feat-login`, working tree limpio → rebase ofrecido. Con un archivo sin commitear → "Rebase" desactivado: "hay cambios sin commitear".

**Fuentes**: Q-CKP-10.

### BR-CKP-ELIG-004: Descartar worktree y rama

**Descripción**: borra el worktree y su rama. Se bloquea con cualquier sesión presente, en el worktree principal, en la rama base y en ramas protegidas. Sin trabajo sin integrar y con snapshot completo, no pide confirmación y muestra el toast Deshacer. Con trabajo sin integrar (commits fuera de la base o cambios sin commitear), ConfirmPrompt con default No que dice qué se pierde. Si hay lo que el snapshot no recupera, aplica BR-CKP-EDGE-008. Con HEAD separado solo se borra el worktree.

**Criticidad**: Alta

**Regla formal**:
```
IF sesión presente OR worktree principal OR rama = base OR rama protegida → bloqueado + motivo
IF HEAD separado → solo borrar el worktree
IF hay lo no recuperable por el snapshot → BR-CKP-EDGE-008
ELSE IF trabajo sin integrar → ConfirmPrompt (default No) con lo que se pierde
ELSE sin confirmación
→ BR-CKP-WF-002 (toast Deshacer)
```

**Ejemplo**: `feat-old` ya integrado en `main`, sin cambios → se descarta sin preguntar; toast "Descartado feat-old · u Deshacer". `feat-wip` con 2 commits no integrados → "Se perderán 2 commits sin integrar (recuperables con Deshacer). ¿Descartar? [y/N]".

**Fuentes**: Q-CKP-12; ADR-TMC-001.

### BR-CKP-ELIG-005: Crear worktree para un agente nuevo

**Descripción**: crea rama y worktree desde la base confirmada, con los parámetros validados (BR-CKP-VAL-001). Se bloquea sin base confirmada. No lanza al agente: al terminar muestra cómo lanzarlo.

**Criticidad**: Media

**Regla formal**:
```
REQUIRE base confirmada                    ELSE desactivado + acción para confirmarla
REQUIRE BR-CKP-VAL-001
→ BR-CKP-WF-002
al terminar → mostrar cómo lanzar el agente en la ruta creada            (BRD § 6.4)
```

**Ejemplo**: se crea `feat/pagos` en `/code/shop-feat-pagos` → "Worktree listo. Para lanzar Claude Code: cd /code/shop-feat-pagos && claude".

**Fuentes**: Q-CKP-13; BRD § 6.4.

### BR-CKP-ELIG-006: Acciones de lectura: ver diff y abrir en el editor

**Descripción**: ver diff y abrir en el editor no escriben en el repo, así que no pasan por la operación protegida ni por Guardrails. Están disponibles en cualquier estado de sesión, salvo con el worktree no disponible. Abrir en el editor es Should.

**Criticidad**: Media

**Regla formal**:
```
IF worktree no disponible → desactivadas + motivo                         (BR-CKP-EDGE-003)
ELSE ofrecidas en cualquier estado de sesión
abrir en editor → BR-CKP-VAL-003; sin editor → BR-CKP-EDGE-007
```

**Ejemplo**: claude-1 Activo en `feat-pagos` → el desarrollador abre el diff y el worktree en su editor sin afectar al agente.

**Fuentes**: Q-CKP-8, Q-CKP-9.

---

## 4. Workflows y Estados

### BR-CKP-WF-001: Estados de sesión y fila de worktree

**Descripción**: la fila es el worktree con sus sesiones (0..n); lo primero que se lee es el nombre del agente. Estados de sesión (heredados): Activo, Inactivo (umbral de 5 min por defecto) y Terminado. Una sesión Terminado sigue visible 24 h o hasta que aparece otra sesión en ese worktree (BR-CKP-TIME-002). El worktree conserva "último agente: X (terminó hace N)" mientras exista. También se listan worktrees sin agente y el principal. Orden: principal fijo arriba; después lo que pide atención (⚡, ⛔, hueco de observación); luego Activo, Inactivo, Terminado y sin agente.

**Criticidad**: Alta

**Regla formal**:
```
fila = worktree + sesiones (0..n) + último agente
sesión: Activo → Inactivo (sin actividad ≥ umbral) → Terminado; Terminado no se reactiva   (Q41)
orden = [principal] + [atención: ⚡ | ⛔ | hueco] + [Activo] + [Inactivo] + [Terminado] + [sin agente]
símbolos: ● Activo · ◐ Inactivo · ○ Terminado (con fallback ASCII)
```

**Ejemplo**: `feat-pagos` con ⚡ sube a la segunda posición aunque su sesión esté Inactiva. `feat-old` con claude-4 Terminado hace 30 h se oculta; con el filtro "ver terminadas" vuelve a verse.

**Fuentes**: Q-CKP-2, Q-CKP-3, Q-CKP-28; BR-WF-001 (motor-local); Q3, Q41 de motor-local; DEP-CKP-4.

### BR-CKP-WF-002: Flujo de una acción de escritura

**Descripción**: toda acción de escritura sigue el mismo flujo. El Cockpit pide la operación del catálogo al daemon. Guardrails decide con la capa `cockpit`. La Time Machine toma el snapshot previo; si falla, no se ejecuta. El ejecutor la ejecuta y se registra. Al terminar, toast con Deshacer (5 s, y después desde el historial).

**Criticidad**: Alta

**Regla formal**:
```
1. intención (operación del catálogo + parámetros) → daemon
2. Guardrails decide (capa cockpit)        denegar → BR-CKP-AUTH-001; nada cambia
3. snapshot previo                         falla → no se ejecuta + motivo   (D-TMC-10)
4. ejecutor revalida precondiciones y ejecuta (serializado por repo)
5. registro de la operación
6. toast "Hecho · u Deshacer" (5 s) + entrada en el historial
```

**Ejemplo**: merge de `feat-pagos` → permitido → snapshot → merge → toast "Integrado feat-pagos en main · u Deshacer". Con el disco lleno, el snapshot falla y el merge no ocurre: "no se pudo tomar el snapshot previo; no se integró nada".

**Fuentes**: Q-CKP-10, Q-CKP-12, Q-CKP-15; D-TMC-10; ADR-TMC-004; ADR-GRP-004 § 3; DEP-CKP-7, DEP-CKP-10.

### BR-CKP-WF-003: Merge o rebase detenido por conflicto

**Descripción**: si el merge o el rebase real choca, queda detenido como lo deja Git. El Cockpit lo muestra con las rutas sin fusionar y ofrece Abortar y Abrir en el editor. Deshacer solo está disponible después de abortar, porque la Time Machine rechaza con una operación en curso. El estado en conflicto no se restaura tal cual. El Cockpit no aborta solo ni resuelve conflictos.

**Criticidad**: Alta

**Regla formal**:
```
merge/rebase → conflicto → estado "detenido en conflicto" + rutas sin fusionar   (DEP-CKP-14)
acciones = { Abortar, Abrir en el editor }
Deshacer disponible ⇔ no hay operación en curso                                  (BR-TMC-EDGE-004)
Constraint: no aborta solo; no resuelve conflictos en la TUI
```

**Ejemplo**: el merge de `feat-pagos` choca en `src/api.rs` → "Merge detenido: 1 archivo en conflicto. [a] Abortar · [e] Editor". Tras abortar, "u Deshacer" vuelve a estar disponible.

**Fuentes**: Q-CKP-11; BR-TMC-EDGE-004; ADR-TMC-002 § 3.1; DEP-CKP-14.

### BR-CKP-WF-004: Estados del motor y de la conexión

**Descripción**: la TUI presenta el estado que publica el motor y el de su conexión con el daemon. Cada estado tiene mensaje y acción. Sin daemon, la TUI lo arranca o explica cómo; nunca embebe el motor.

**Criticidad**: Alta

| Estado | Qué ve el desarrollador |
|---|---|
| Esperando Git | Qué falta, versión mínima y cómo instalarla (BR-WF-002 (motor-local)) |
| Sin repos | Estado vacío guiado: cómo añadir el primer repo |
| Observando | Vista normal |
| Reconciliando | Vista con aviso "reconciliando"; los datos pueden cambiar |
| Observación degradada | Aviso y guía (p. ej. límite de inotify) |
| Resync (cliente lento) | La TUI vuelve a pedir la instantánea y lo indica (SEC-08) |
| Sin daemon | Arranque automático o instrucciones (ADR-GRP-005 § 3) |
| Hueco de observación | Destacado en la fila y en la lista de atención (SEC-13) |

**Regla formal**:
```
estado mostrado = estado publicado por el motor
"Esperando Git" tiene prioridad sobre "Sin repos"                       (BR-WF-002 (motor-local))
sin daemon → autoarranque (DEP-CKP-12) OR instrucciones; nunca motor embebido
resync → nueva instantánea + suscripción coherente                      (DEP-CKP-6)
```

**Ejemplo**: en una máquina nueva sin repos la TUI muestra "Aún no observas ningún repo. Añade uno con raptor repo add <ruta>".

**Fuentes**: Q-CKP-22; BR-WF-002 (motor-local); SEC-08, SEC-13; DEP-CKP-6, DEP-CKP-12.

### BR-CKP-WF-005: Rama base no confirmada o pendiente

**Descripción**: con la base no confirmada o un cambio pendiente de confirmar, la predicción contra la base se muestra "pendiente". Merge, rebase y crear worktree quedan desactivados con el motivo y la acción para confirmar. Los solapes y conflictos entre worktrees siguen.

**Criticidad**: Alta

**Regla formal**:
```
IF base ∈ {no confirmada, pendiente}:
  pares (worktree, base) → "pendiente"
  merge, rebase, crear worktree → desactivados + "confirma la rama base" + acción
  pares (worktree, worktree) → sin cambios
```

**Ejemplo**: el equipo cambió la base a `develop` y aún no se confirmó → "Rama base pendiente de confirmar (main → develop)". El ⚡ entre claude-1 y claude-2 sigue visible.

**Fuentes**: Q-CKP-27; Q-GRD-21, Q-GRD-23, Q-GRD-25.

### BR-CKP-WF-006: Cola de confirmación (cuando exista)

**Descripción**: el Cockpit presentará la cola de Guardrails: peticiones pendientes con cuenta atrás, actor y regla. Aprobar solo con el factor OS; rechazar es comando reservado. Mientras Guardrails no publique la cola, "pedir confirmación" equivale a denegar y no hay cola que mostrar. La historia queda bloqueada y no se planifica en el MVP del Cockpit.

**Criticidad**: Media

**Regla formal**:
```
IF cola no publicada (DEP-CKP-8) → no se muestra; "pedir confirmación" = denegar   (S-GRD-9)
ELSE FOR EACH petición pendiente: actor + operación + regla + cuenta atrás (BR-CKP-TIME-003)
     aprobar → BR-CKP-AUTH-004 ; rechazar → BR-CKP-AUTH-004
```

**Ejemplo**: claude-2 pide un rebase con "pedir confirmación" → "claude-2 · rebase feat-login · regla rebase=ask · 4:12".

**Fuentes**: Q-CKP-14; BR-WF-001 (Guardrails); Q-GRD-19; S-GRD-9; DEP-CKP-8.

### BR-CKP-WF-007: Alertas de conflicto dentro de la TUI

**Descripción**: al aparecer un ⚡ nuevo, la TUI muestra un ConflictAlert y un toast. No hay notificación del sistema operativo ni campana en el MVP.

**Criticidad**: Media

**Regla formal**:
```
IF aparece ⚡ para un par que no lo tenía → ConflictAlert + toast
Constraint: sin notificación del SO ni campana (Won't en el MVP)
```

**Ejemplo**: claude-2 commitea un cambio que choca con claude-1 → toast "⚡ Conflicto previsto: claude-1 ↔ claude-2 en src/api.rs".

**Fuentes**: Q-CKP-7.

---

## 5. Permisos y Autorizaciones

### BR-CKP-AUTH-001: El Cockpit no autoriza; decide Guardrails

**Descripción**: toda acción de escritura pasa por la decisión de Guardrails (capa `cockpit`). Si se deniega, se muestra un PolicyBanner ⛔ con la regla y el nivel. Se ofrece la excepción consciente de ADR-GRD-007: anuncio, ventana cancelable y auditoría. Si la TUI desciende de un agente, la excepción se rechaza y se explica. Sin cola publicada, "pedir confirmación" se trata como denegar. Un merge a una rama protegida por el equipo pasa siempre por excepción consciente.

**Criticidad**: Alta

**Regla formal**:
```
decisión = Guardrails(operación, solicitante, capa = cockpit)          (DEP-CKP-10)
IF decisión ∈ {denegar, pedir confirmación sin cola} → PolicyBanner ⛔ (regla + nivel)
  ofrecer excepción consciente UNLESS solicitante desciende de un agente
  excepción → anuncio + ventana 10 s (S-CKP-3) + auditoría → BR-CKP-WF-002
IF permitir → BR-CKP-WF-002
```

**Ejemplo**: merge a `main`, protegida por el equipo → "⛔ main es rama protegida (equipo). [x] Excepción consciente". Tras confirmar, cuenta atrás de 10 s cancelable; al terminar, el merge sigue el flujo normal y queda auditado.

**Fuentes**: Q-CKP-15, Q-CKP-26; Q-GRD-1, Q-GRD-24; ADR-GRD-007; S-GRD-9.

### BR-CKP-AUTH-002: Solicitante por ascendencia

**Descripción**: el solicitante lo resuelve el daemon por la ascendencia del proceso. Una TUI lanzada desde el terminal de un agente actúa como ese agente y la vista lo declara. La confirmación en la TUI es UX, no control.

**Criticidad**: Alta

**Regla formal**:
```
solicitante = daemon.resolver_por_ascendencia(proceso de la TUI)        (ADR-TMC-005 § 1)
IF solicitante = agente X → la vista muestra "actúas como X"
Constraint: una confirmación en la TUI no cambia el solicitante ni autoriza nada
```

**Ejemplo**: el desarrollador abre `raptor` desde la pestaña donde corre claude-1 → cabecera "Actúas como claude-1"; la excepción consciente no se ofrece.

**Fuentes**: Q-CKP-16; ADR-TMC-005 § 1; ADR-GRP-005 § 6.

### BR-CKP-AUTH-003: Merge o descarte sobre trabajo de otro actor

**Descripción**: si un merge o un descarte afecta trabajo de un actor distinto del solicitante, se extiende la regla de la Time Machine: confirmación interactiva ligada al plan concreto. En Windows, sin esa confirmación, se rechaza.

**Criticidad**: Alta

**Regla formal**:
```
IF el trabajo afectado es de un actor ≠ solicitante:
  IF solicitante = agente → rechazo
  IF sistema = Windows → rechazo + motivo                               (como TQ-14)
  ELSE confirmación interactiva ligada al plan; sin ella, rechazo     (ADR-CKP-002)
```

**Ejemplo**: en macOS, "Tú u otro (sin atribuir)" descarta `feat-wip` de claude-2 → se pide confirmar el plan "borrar worktree feat-wip y rama feat-wip". En Windows → "no se puede confirmar trabajo de otro actor en Windows todavía".

**Fuentes**: Q-CKP-16; ADR-TMC-005 § 2-3; BR-TMC-AUTH-001; DEP-CKP-7.

### BR-CKP-AUTH-004: Aprobar y rechazar en la cola

**Descripción**: aprobar una petición de la cola solo se ofrece con el factor de autenticación del sistema operativo. Rechazar es fail-safe y es un comando reservado, sin ventana.

**Criticidad**: Alta

**Regla formal**:
```
aprobar  → REQUIRE factor OS (Q-GRD-19); sin factor → no se ofrece
rechazar → comando reservado (ADR-GRP-005 § 6), sin ventana
```

**Ejemplo**: sin factor OS disponible, la petición muestra solo "[r] Rechazar".

**Fuentes**: Q-CKP-14; Q-GRD-19; ADR-GRP-005 § 6.

---

## 6. Reglas de Consistencia de Datos

### BR-CKP-CONS-001: Fuente única: el motor

**Descripción**: todo lo que la TUI muestra sale de lo que el motor publica. La TUI no lee Git, no abre el perfil y no calcula estado propio.

**Criticidad**: Alta

**Regla formal**:
```
dato mostrado ∈ publicado por el motor (instantánea + eventos + consultas bajo demanda)
Constraint: la TUI no lee Git ni el perfil
```

**Ejemplo**: si el motor está "Reconciliando", la TUI no corre `git status` para adelantarse: muestra el aviso.

**Fuentes**: ADR-GRP-005, ADR-GRP-013.

### BR-CKP-CONS-002: Una sola vía de escritura y nunca push

**Descripción**: el Cockpit escribe solo con operaciones del catálogo ejecutadas por el daemon como operación protegida. No hay otra vía. Ninguna operación del Cockpit contacta con el remoto.

**Criticidad**: Alta

**Regla formal**:
```
escrituras del Cockpit ⊆ catálogo de operaciones (ejecutor del daemon)   (DEP-CKP-7)
Constraint: cada escritura es operación protegida                         (ADR-TMC-004)
Constraint: nunca push ni fetch
```

**Ejemplo**: tras integrar `feat-pagos` en `main`, el ahead de `main` frente al remoto sube; el Cockpit no ofrece push.

**Fuentes**: Q-CKP-10; ADR-TMC-004; NFR-01.

### BR-CKP-CONS-003: Presentación del actor

**Descripción**: el actor se presenta como "agente X" (detectado o registrado) o "Tú u otro (sin atribuir)". Nunca como "humano".

**Criticidad**: Alta

**Regla formal**:
```
actor mostrado ∈ { "agente X (detectado|registrado)", "Tú u otro (sin atribuir)" }
```

**Ejemplo**: un commit hecho desde una terminal sin agente detectado aparece como "Tú u otro (sin atribuir)".

**Fuentes**: Q34, Q35 de motor-local; D-TMC-12.

### BR-CKP-CONS-004: Varias TUIs y serialización

**Descripción**: se permiten varias TUIs a la vez y todas ven el mismo estado. El ejecutor serializa las operaciones por repo y revalida las precondiciones con el valor anterior esperado. Una acción anunciada en una TUI se ve en todas.

**Criticidad**: Alta

**Regla formal**:
```
operaciones del mismo repo → en serie
al ejecutar: IF estado actual ≠ estado esperado por la TUI → rechazo "el estado cambió" ; nada cambia
acción anunciada (reserved-action-pending) → visible en todas las TUIs
```

**Ejemplo**: dos TUIs piden descartar `feat-old` a la vez → la primera se ejecuta; la segunda recibe "el worktree ya no existe".

**Fuentes**: Q-CKP-19.

### BR-CKP-CONS-005: Registro de predicciones para el KPI

**Descripción**: el daemon registra, en el perfil y por repo, cada (par, archivo) en su primera aparición como conflicto previsto, y cada conflicto real. Se conserva 90 días. "Detectado antes" significa que había un ⚡ (no solo un solape) del mismo par y archivo antes de empezar la operación que chocó. Los conflictos ocurridos en huecos de observación o fuera del repo local se muestran aparte y no entran. Métrica complementaria: conflictos previstos que no ocurrieron. La consulta es local.

**Criticidad**: Media

**Regla formal**:
```
registrar (par, archivo, hora) en la primera aparición de ⚡
registrar conflicto real (par, archivo, hora inicio de la operación)      (DEP-CKP-14)
detectado_antes ⇔ ∃ ⚡(par, archivo) con hora < inicio de la operación
EXCLUDE conflictos en huecos de observación o en el remoto → listados aparte
KPI = detectados_antes / conflictos_reales_incluidos ; complementaria = ⚡ que no ocurrieron
retención 90 días, en el perfil, por repo; consulta local (NFR-03)
```

**Ejemplo**: ⚡ claude-1 ↔ claude-2 en `src/api.rs` a las 10:00; el merge de las 11:00 choca en ese archivo → cuenta como detectado antes. Un conflicto en `README.md` que solo tuvo ⚠ → no cuenta.

**Fuentes**: Q-CKP-21, Q-CKP-25; ADR-GRD-006 (forma); DEP-CKP-11, DEP-CKP-14.

### BR-CKP-CONS-006: Preferencias de la TUI en el perfil, vía daemon

**Descripción**: repo seleccionado, panel, filtros y layout se guardan por usuario en el perfil, a través del daemon, que es el único escritor del perfil. Nunca en el repo.

**Criticidad**: Baja

**Regla formal**:
```
preferencias → daemon → perfil (por usuario)                             (DEP-CKP-11)
arranque → repo del directorio actual ?? último usado
```

**Ejemplo**: el desarrollador cierra la TUI con el filtro "ver terminadas" activo; al volver, sigue activo.

**Fuentes**: Q-CKP-1, Q-CKP-17.

### BR-CKP-CONS-007: CLI de solo lectura

**Descripción**: `raptor status` y `raptor conflicts` muestran el mismo estado que la TUI, con `--json`, sin mensajes de commit ni contenido de diff. Las acciones de BR-07 no existen en la CLI del MVP.

**Criticidad**: Baja

**Regla formal**:
```
status, conflicts → solo lectura; --json disponible
Constraint: sin mensajes de commit ni contenido (restricciones del MCP)
```

**Ejemplo**: `raptor conflicts --json` lista pares, nivel, archivos y antigüedad, sin hunks de contenido.

**Fuentes**: Q-CKP-20.

---

## 7. Reglas de Tiempo y Expiración

### BR-CKP-TIME-001: La TUI pinta en ≤ 100 ms p95

**Descripción**: desde que la TUI recibe un evento hasta que lo pinta pasan ≤ 100 ms p95, dentro de los < 500 ms de extremo a extremo de NFR-04, con 10 worktrees y 100K commits. Lo verifica un gate de CI sin pantalla. El arranque y la reconciliación quedan fuera del gate, pero se muestran como estado.

**Criticidad**: Alta

**Regla formal**:
```
t_render − t_client_recv ≤ 100 ms (p95)                                (ADR-GRP-011)
e2e < 500 ms (p95), 10 worktrees, 100K commits                         (NFR-04, NFR-05)
```

**Ejemplo**: claude-1 guarda un archivo; la fila de `feat-pagos` cambia su recuento de archivos en menos de medio segundo.

**Fuentes**: ADR-GRP-011; NFR-04, NFR-05; INF-GRP-002.

### BR-CKP-TIME-002: Sesión Terminado visible 24 h

**Descripción**: una sesión Terminado sigue visible en su fila 24 h o hasta que aparece otra sesión en ese worktree, lo que ocurra antes. Después se oculta por defecto.

**Criticidad**: Baja

**Regla formal**:
```
visible ⇔ ahora − fin < 24 h AND no hay sesión posterior en el worktree
oculta → visible con el filtro "ver terminadas"
```

**Ejemplo**: claude-4 terminó a las 9:00 del lunes → visible hasta las 9:00 del martes, salvo que antes se abra otra sesión en ese worktree.

**Fuentes**: Q-CKP-3.

### BR-CKP-TIME-003: Cuenta atrás de la cola

**Descripción**: cada petición pendiente muestra su cuenta atrás hasta la caducidad heredada de Guardrails (5 min por defecto). Al caducar, deja de ofrecer acciones.

**Criticidad**: Media

**Regla formal**:
```
restante = caducidad (Guardrails, 5 min por defecto) − antigüedad de la petición
IF restante ≤ 0 → "caducada"; sin acciones
```

**Ejemplo**: petición creada a las 10:00 → a las 10:03 muestra "2:00"; a las 10:05, "caducada".

**Fuentes**: Q-CKP-14; Q-GRD-6.

### BR-CKP-TIME-004: Aviso de purga de la Time Machine

**Descripción**: la TUI presenta el aviso de purga de snapshots de la Time Machine. Mostrarlo cuenta como "visto" para la regla de purga (aviso visto + 24 h).

**Criticidad**: Media

**Regla formal**:
```
IF la Time Machine publica aviso de purga → mostrarlo + notificar "visto"
```

**Ejemplo**: al abrir la TUI aparece "3 snapshots de más de 30 días se purgarán a partir de mañana a las 10:00".

**Fuentes**: BR-TMC-TIME-001.

---

## 8. Reglas Excepcionales (Edge Cases)

### BR-CKP-EDGE-001: Rama base no calculable

**Descripción**: si la rama base no existe en el repo, ahead/behind, carriles y pares contra la base se muestran "no calculable" con el motivo. Nunca se elige otra rama.

**Criticidad**: Alta

**Ejemplo**: la base es `develop` y no existe localmente → "Rama base develop no encontrada: no calculable". Los pares entre worktrees siguen.

**Fuentes**: Q42 de motor-local.

### BR-CKP-EDGE-002: HEAD separado u operación en curso

**Descripción**: un worktree con HEAD separado se muestra como tal; descartar solo borra el worktree. Con una operación de Git en curso (merge, rebase), se muestra el estado y se desactivan merge, rebase, descartar y Deshacer hasta abortar o terminar.

**Criticidad**: Alta

**Ejemplo**: `feat-login` con un rebase a medias hecho por el agente → "rebase en curso"; solo se ofrecen ver diff, abrir en el editor y, si lo inició el Cockpit, Abortar.

**Fuentes**: Q-CKP-11, Q-CKP-12; BR-TMC-EDGE-004.

### BR-CKP-EDGE-003: Worktree no disponible

**Descripción**: si el motor publica un worktree como no disponible (borrado o movido fuera de GitRaptor), la fila lo indica y todas las acciones quedan desactivadas.

**Criticidad**: Media

**Ejemplo**: el directorio de `feat-x` se borró a mano → "no disponible"; sin acciones.

**Fuentes**: BR-EDGE-001 (motor-local).

### BR-CKP-EDGE-004: Worktree compartido

**Descripción**: un worktree con varias sesiones se marca como compartido y muestra todas. El diff no se atribuye por archivo. Las precondiciones de sesión (BR-CKP-ELIG-001) se evalúan con la sesión más restrictiva.

**Criticidad**: Media

**Ejemplo**: claude-1 Terminado y claude-2 Inactivo en `feat-pagos` → "compartido"; el rebase sigue bloqueado por claude-2.

**Fuentes**: Q-CKP-2, Q-CKP-8; Q7 de motor-local.

### BR-CKP-EDGE-005: Terminal menor de 80×24

**Descripción**: con 80×24 o más, el espacio se reparte por prioridad: lista > alertas de conflicto y política > grafo > detalle; el grafo colapsa primero. Por debajo de 80×24 se muestra solo un mensaje de tamaño mínimo.

**Criticidad**: Media

**Ejemplo**: terminal de 100×30 → lista, alertas y grafo reducido; de 70×20 → "Amplía la terminal a 80×24 como mínimo".

**Fuentes**: Q-CKP-18; NFR-09.

### BR-CKP-EDGE-006: Más de 8 agentes

**Descripción**: los colores `agent.1..8` se reutilizan a partir del noveno agente. El nombre y el símbolo distinguen siempre; el color nunca es la única señal.

**Criticidad**: Baja

**Ejemplo**: claude-9 comparte color con claude-1, pero su nombre aparece en la fila y en su carril.

**Fuentes**: Q-CKP-23; NFR-09.

### BR-CKP-EDGE-007: Editor ausente

**Descripción**: si no hay editor configurado ni `$VISUAL`/`$EDITOR`, abrir en el editor devuelve un error accionable que dice cómo configurarlo. Un editor de terminal suspende la TUI y la restaura al volver; uno gráfico no la bloquea.

**Criticidad**: Baja

**Ejemplo**: sin editor → "No hay editor configurado. Define $EDITOR o la clave del editor en tu perfil".

**Fuentes**: Q-CKP-9.

### BR-CKP-EDGE-008: Descarte con lo que el snapshot no recupera

**Descripción**: si el worktree contiene lo que el snapshot no guarda (ignorados como `.env` o `node_modules`, archivos de credenciales, submódulos, archivos excluidos por tamaño), la confirmación es obligatoria y nombra lo que no será recuperable. No se promete un Deshacer total.

**Criticidad**: Alta

**Regla formal**:
```
no_recuperable = ignorados ∪ credenciales excluidas ∪ submódulos ∪ excluidos por tamaño   (ADR-TMC-001)
IF no_recuperable ≠ ∅ → ConfirmPrompt obligatorio (default No) que lista no_recuperable
mensaje de Deshacer → "recupera todo salvo lo listado"
```

**Ejemplo**: `feat-wip` tiene `.env.local` y `node_modules/` → "No se podrán recuperar: .env.local, node_modules/. ¿Descartar? [y/N]".

**Fuentes**: Q-CKP-12; ADR-TMC-001; BR-TMC-CONS-002.

### BR-CKP-EDGE-009: Ningún worktree tiene la base sacada

**Descripción**: si ningún worktree tiene la rama base sacada, el merge solo se ofrece si es fast-forward sin worktree; si no, se rechaza con el motivo. Lo fija ADR-CKP-002.

**Criticidad**: Media

**Ejemplo**: todos los worktrees están en ramas de agentes → "Merge no fast-forward: no hay ningún worktree con main sacada. Saca main en un worktree para integrar".

**Fuentes**: Q-CKP-10; DEP-CKP-7.

---

## Matriz de Priorización

Alineada con el orden de entrega Q-CKP-24: 1) BR-04 con estados del motor; 2) BR-06; 3) BR-07; 4) BR-05.

| Regla | Criticidad | Prioridad |
|-------|------------|-----------|
| CONS-001, CONS-003, WF-001, WF-004, CALC-001, TIME-001, VAL-002, EDGE-001, EDGE-003 | Alta/Media | 🔴 P0 (BR-04) |
| CALC-002, CALC-003, WF-005, WF-007, CONS-005 | Alta/Media | 🔴 P0 (BR-06) |
| WF-002, CONS-002, CONS-004, AUTH-001, AUTH-002, AUTH-003, ELIG-001 a ELIG-004, WF-003, EDGE-002, EDGE-008, EDGE-009 | Alta | 🟡 P1 (BR-07, escritura) |
| VAL-001, ELIG-005, ELIG-006, CALC-005, TIME-004, EDGE-004, EDGE-005 | Alta/Media | 🟡 P1 (BR-07, resto) |
| CALC-004 | Media | 🟢 P2 (BR-05, depende de DEP-CKP-2) |
| VAL-003, EDGE-007, CONS-006, CONS-007, TIME-002, EDGE-006 | Media/Baja | 🟢 P2 (Should y comodidad) |
| WF-006, AUTH-004, TIME-003 | Alta/Media | ⏸ Bloqueadas por DEP-CKP-8 |

Los IDs omiten el prefijo `BR-CKP-`.

---

## Trazabilidad

### Reglas → Capacidades del BRD

| Capacidad | Reglas |
|-----------|--------|
| BR-04 Lista en vivo | CALC-001, WF-001, WF-004, WF-005, CONS-001, CONS-003, CONS-004, CONS-006, CONS-007, TIME-001, TIME-002, VAL-002, EDGE-001, EDGE-003, EDGE-004, EDGE-005, EDGE-006 |
| BR-05 Grafo en vivo | CALC-004, EDGE-001, EDGE-005 |
| BR-06 Predicción de conflictos | CALC-002, CALC-003, WF-005, WF-007, CONS-005, CONS-007 |
| BR-07 Acciones por agente | VAL-001, VAL-003, CALC-005, ELIG-001 a ELIG-006, WF-002, WF-003, WF-006, AUTH-001 a AUTH-004, CONS-002, CONS-004, TIME-003, TIME-004, EDGE-002, EDGE-007, EDGE-008, EDGE-009 |

### Reglas → User Stories

Pendiente: las historias se generan tras la aprobación del requerimiento (ADR-018).

### Reglas → Criterios de Aceptación

Cada regla tendrá al menos un escenario Gherkin, incluido uno negativo, en su historia. La demo del BRD § 13 es prueba de aceptación del feature (Q-CKP-25).

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 0.1 | 2026-10-04 | PO (AADD) | Versión inicial: 45 reglas a partir de Q-CKP-1 a Q-CKP-30 (decisión del orquestador, validada por PO y Arquitecto) y de las decisiones heredadas de motor-local, Time Machine y Guardrails. |
