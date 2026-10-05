---
id: ADR-GRP-013
title: Modelo persistido de eventos, sesiones y atribución
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-03
created: 2026-10-03
updated: 2026-10-05
deciders: [Rene Bonilla]
domain: GRP
feature: motor-local
related: [ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-012, ADR-GRD-006, ADR-GRD-007, ADR-CKP-001, ADR-CKP-002, CTX-GRP-001, BR-GRP-001]
tags: [motor-local, eventos, sesiones, atribucion, correccion, huecos, append-only, modelo-de-datos, auditoria, no-repudio, seguridad]
---

# ADR-GRP-013 — Modelo persistido de eventos, sesiones y atribución

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

## Contexto

El motor guarda en el perfil (ADR-GRP-006) los eventos de Git, las sesiones de agente, los registros explícitos y las correcciones. Las reglas de negocio fijan cómo se comportan:

- El motor solo emite "agente X" con su origen ("detectado" o "registrado") o "sin atribuir". **Nunca "humano"** (Q34) ni "no identificada" (Q35), y ante la duda no atribuye (BR-EDGE-004).
- **Corregir** reemplaza la atribución detectada de una sesión, no añade sesiones (Q33) y alcanza todos los eventos de esa sesión **desde su inicio**, sin tocar los de otras sesiones (Q37). Solo se corrige donde hay una atribución detectada (Q38). Una detección posterior no deshace una corrección vigente (BR-CONS-002).
- **Registrar otro agente**, si ya hay otra sesión presente, añade una sesión y el worktree pasa a compartido; registrar al mismo agente ya detectado **confirma** esa sesión (Q39, BR-CONS-004).
- Una sesión terminada no se reactiva, y un agente registrado figura presente hasta que se **retira su registro**, lo que termina su sesión (Q41, BR-WF-001).
- Lo que el motor no vio ocurrir queda "sin atribuir", aunque hubiera un agente registrado antes del hueco (BR-EDGE-005).
- Todo sobrevive a reinicios (Q6, BR-CONS-005).
- Abiertas para el PO: **P16** (¿una sesión confirmada por registro pasa a origen "registrado"?) y **P17** (¿al retirar una corrección, sus eventos vuelven a la atribución detectada?).

## Decisión

**Cada evento apunta a una sesión o a ninguna ("sin atribuir"). La atribución se resuelve desde la sesión, a partir de su atribución inicial y de una cadena append-only de registros de atribución. Los eventos nunca se reescriben ni se borran.**

### 1. Entidades (por repo, en el almacén del repo de ADR-GRP-006)

| Entidad | Qué guarda |
|---------|------------|
| **Worktree** | Ruta canónica, nombre administrativo en Git, primera vez visto y, si desapareció, cuándo. |
| **Sesión** | Worktree, agente (Claude Code u "otro agente" con su nombre declarado), atribución inicial y su origen (detectada o registrada), clave de detección de ADR-GRP-012 si se detectó, inicio, fin y causa del fin (proceso desaparecido, terminada durante un hueco o **registro retirado**). |
| **Registro de atribución** | Append-only. Sesión, tipo (registro, confirmación, corrección, retiro de corrección, **retiro de registro**), agente, autor (desarrollador o agente, como exige la tabla de BR-CONS-001), momento y secuencia en la que entra en vigor. |
| **Evento** | Secuencia por repo, worktree, tipo, metadatos (refs, ids de commit, rutas afectadas), hora de observación en UTC con el desfase de zona horaria local, sesión (opcional), evidencia de atribución (qué señales de ADR-GRP-012 la sustentan) y hueco (opcional). |
| **Hueco** | Intervalo de inicio y fin y su causa: máquina apagada o suspendida, daemon caído, **daemon caído durante una sesión activa**, daemon parado por un comando, repo retirado, Git ausente o insuficiente (BR-WF-002), perfil perdido o almacén corrupto, y las causas de observación de ADR-GRP-010 § 6: desbordamiento de la cola del watcher, recreación del stream del watcher y reconciliación periódica (diferencias sin causa identificada) (Enmienda 2026-10-04, SPIKE-GRP-002). Si lo provocó un comando (parada, retiro del repo), guarda **el cliente que lo pidió** (proceso, ejecutable y si pasó los controles de ADR-GRP-005) (SEC-13). |
| **Último estado conocido** | Por worktree: HEAD, puntas de refs, operación en curso y huella de los cambios sin commitear. Es la base de la reconciliación (ADR-GRP-010). (Enmienda 2026-10-04, Cockpit: más el estado en conflicto; ver la sección final.) |
| **Marca "observado hasta"** | Momento hasta el que el repo estuvo observado, persistido con cada lote y de forma periódica. |

Los cambios de estado de las sesiones (inicio, activo, inactivo, terminado) se guardan como eventos del mismo historial, así que su estado sobrevive a reinicios (BR-WF-001).

**Registro de auditoría de comandos reservados (SEC-03)**: fuera de los almacenes por repo, en el índice global del perfil (ADR-GRP-006), porque algunos comandos (parar el daemon, añadir un repo) no pertenecen a un repo. Es **append-only**: cada intento de comando reservado de ADR-GRP-005 § 6 (añadir o retirar repo, corregir o retirar corrección, retirar el registro de otro agente, parar el daemon), aceptado o rechazado, con fecha, operación, repo si aplica, resultado, motivo del rechazo y cliente (ejecutable y si desciende de un agente). Nunca se reescribe ni se borra; los clientes lo pueden consultar.

**Comandos reservados de Guardrails en la auditoría (Enmienda 2026-10-04; ADR-GRD-006 § 4, ADR-GRD-007)**. El mismo registro append-only y sin caducidad incluye:

- **Qué entra**: cada intento de comando reservado de Guardrails (ADR-GRP-005 § 6), aceptado o rechazado; la instalación, la desinstalación, la adopción y el refresco de integridad; cada uso de la excepción consciente y cada rechazo (`exception-rejected`); el anuncio, la cancelación y la aplicación de las acciones con ventana (D5).
- **Con qué datos**, además de los de arriba: la **cadena completa de ascendencia** (ruta del ejecutable e identificador de cada proceso), la **terminal de control** y el **líder de sesión**; y, en cada acción que relaja, la **aceptación del riesgo por acción**: la referencia a la versión de la tabla de vectores de ADR-GRD-007 § 2 y los vectores no cubiertos en ese momento.
- **Cómo**: siempre con la operación normalizada, nunca con argv ni con el token de la excepción (M-06; ADR-GRD-007 § 3).

### 2. Resolución de la atribución

- **Actor de un evento**: si no tiene sesión, "sin atribuir". Si la tiene, la atribución efectiva de esa sesión.
- **Atribución efectiva de una sesión**: se parte de la atribución inicial y se aplican en orden los registros vigentes:
  - **Confirmación** (registro del mismo agente ya detectado, Q39): misma sesión y mismo agente. **No activa la evidencia por registro de § 3**: la sesión sigue atribuyendo solo con S2b, S3 o S4. ⚠️ **ASSUMPTION** dependiente de **P16** (supuesto: sí): el origen pasa a "registrado".
  - **Corrección**: agente corregido con origen "registrado". Como todos los eventos de la sesión apuntan a ella, la corrección alcanza **desde el inicio de la sesión** (Q37) y no toca los eventos de otras sesiones. No crea sesiones ni marca el worktree como compartido (Q33).
  - **Retiro de registro** (Q41, BR-WF-001): no cambia la atribución de la sesión; la termina con causa "registro retirado" y deja de ser presente, así que deja de contar para decidir si el worktree es compartido (§ 3). Una sesión terminada no se reabre.
  - **Retiro de corrección**: deja sin efecto la corrección retirada y vuelve la atribución anterior. ⚠️ **ASSUMPTION** dependiente de **P17** (supuesto: sí): los eventos que la corrección había reatribuido vuelven también, porque se resuelven desde la sesión.
- **Precondiciones de la corrección** (las aplica el daemon): la pide el desarrollador (BR-AUTH-001, controles de ADR-GRP-005) y la sesión tiene atribución inicial detectada; si en el worktree no hay ninguna, se rechaza e indica que se use el registro (Q38).
- **Autorización del retiro de registro** (la aplica el daemon con los controles de ADR-GRP-005 § 6). ⚠️ **ASSUMPTION** pendiente de confirmar por Rene: el desarrollador puede retirar cualquier registro; un agente solo el suyo, es decir, el de la sesión que él mismo registró en el worktree que es su cwd (la misma regla que para registrarse). Un agente que intenta retirar otro registro recibe un rechazo que queda en el registro de auditoría.
- **La detección no escribe registros**: actualizar la presencia o la actividad de una sesión detectada no cambia su atribución, así que una detección posterior no deshace una corrección vigente (BR-CONS-002).
- **Si el PO responde "no" a P16 o P17**, el modelo no cambia de forma: la confirmación deja el origen como está (P16), y el retiro de una corrección solo vale para los eventos con secuencia posterior al retiro, gracias a la secuencia de entrada en vigor de cada registro (P17).

### 3. Asignación de sesión a un evento

- Un evento solo apunta a una sesión con **evidencia positiva** de ADR-GRP-012 (proceso, ascendencia, contenido de sesión o hooks existentes). La co-ubicación de una sesión detectada sola no basta. Ante la duda, sin sesión (BR-EDGE-004).
- **El registro explícito es evidencia positiva** (BR-VAL-001, ejemplo 3 de BR-EDGE-004, US-GRP-009): mientras una sesión registrada sea **la única sesión presente** en el worktree, los eventos del worktree apuntan a ella, aunque no haya S2b, S3 ni S4. Así un agente sin detección ("otro agente: Codex") recibe sus commits. **Solo aplica a sesiones creadas por el registro de un agente sin detección automática** ("otro agente"). Una sesión detectada y luego confirmada (Q39) no gana esta evidencia, aunque con P16 su origen pase a "registrado": sigue con S2b, S3 y S4, y las ediciones del humano en su worktree nunca se atribuyen a Claude Code (US-GRP-008, BR-EDGE-004).
- En un **worktree compartido** (varias sesiones presentes, detectadas o registradas) el registro deja de bastar: solo se asigna una sesión si la evidencia por evento (S2b, S3 o S4) identifica una sola; ante la duda, "sin atribuir". En el MVP no hay atribución por archivo dentro de un worktree compartido (Q7, BR-CONS-004).
- Que un worktree sea compartido se deriva de cuántas sesiones presentes tiene; no se guarda como dato propio.
- Una sesión terminada no se reabre: si el agente vuelve, es una sesión nueva (Q41).

### 4. Orden, tiempo e inmutabilidad

- **Secuencia monotónica por repo**, asignada por el único escritor (ADR-GRP-005). El orden lo da la secuencia, no el reloj, para resistir saltos de hora.
- La hora de observación se guarda en UTC con el desfase local. Las fechas propias de Git (autor y commit) se guardan como metadatos, sin usarlas para ordenar.
- **Los eventos nunca se borran ni se reescriben**, tampoco al retirar un repo (Q25). Las correcciones y sus retiros quedan como rastro en los registros de atribución.
- Solo se registran como eventos los cambios ocurridos desde que el repo se observa: la historia previa de Git no se importa como eventos.

### 5. Huecos y reconciliación

- Al arrancar, al volver de suspensión, al volver a añadir un repo y al salir de "Esperando Git", el motor abre un hueco desde la marca "observado hasta" hasta ese momento, con su causa.
- **Huecos del observador** (Enmienda 2026-10-04, SPIKE-GRP-002): un desbordamiento o una recreación del stream abren un hueco desde el último evento recibido antes de la causa hasta el fin de la reconciliación que la sigue. La reconciliación periódica (ADR-GRP-010 § 5) solo abre hueco si encuentra diferencias, y su intervalo va desde la reconciliación anterior del worktree hasta ahora. Los tres guardan su causa como los demás.
- La reconciliación (ADR-GRP-010) compara el estado actual con el último estado conocido y genera **eventos de reconciliación sin sesión y enlazados al hueco**. Nunca se atribuyen a un agente, aunque hubiera uno registrado antes (BR-EDGE-005).
- Sin último estado conocido (perfil perdido, almacén corrupto o máquina nueva), el repo expone "observado desde" su fecha de alta y todo lo anterior queda "sin atribuir" (Q26, Q31).
- Una sesión cuyo proceso desapareció durante un hueco se cierra con causa "terminada durante un hueco" y hora de fin desconocida dentro del intervalo.
- **No repudio (SEC-13)**: si el daemon se para o cae mientras hay alguna sesión activa, el hueco se marca como tal y los clientes lo muestran de forma destacada. La causa de una parada viene de la parada ordenada (con su comando y cliente) o, si no hubo parada ordenada, se registra como caída. Un agente no puede dejar su trabajo "sin atribuir" sin dejar rastro.

### 6. Lo que expone el contrato (`crates/api`)

- El actor expuesto solo tiene dos variantes: **agente** (tipo, nombre y origen) o **sin atribuir**. El tipo no tiene variante "humano", así que el motor no puede emitirla.
- Al aplicar una confirmación, una corrección o un retiro, el stream de eventos publica que la atribución de esa sesión cambió, con el rango de secuencias afectado, para que la Time Machine (F-001-03) y Guardrails (F-001-04) refresquen.
- Consultas por worktree, por sesión, por rango de secuencia y por actor efectivo, y consulta del registro de auditoría y de los huecos con su causa.
- **Texto no confiable (SEC-12)**: el nombre declarado de un agente, las rutas y las refs son texto procedente del repo o de un agente; el contrato lo marca como tal y los clientes lo limpian antes de mostrarlo (ADR-GRP-005 § 5). Las respuestas al MCP no incluyen mensajes de commit ni contenido y se limitan al repo del llamante.
- **Última actividad, última sesión y estado en conflicto por worktree** (Enmienda 2026-10-04, Cockpit): ver la sección final.

## Alternativas consideradas

| Alternativa | Por qué no |
|-------------|------------|
| **Atribución copiada en cada evento; una corrección reescribe los eventos afectados** | Mutación masiva en cada corrección, sin rastro de la corrección, y retirar una corrección obliga a recordar qué valía antes cada evento. |
| **Event sourcing completo con proyecciones** | Más potente, pero desproporcionado para el MVP: reconstruir proyecciones a escala de NFR-05 añade complejidad sin un caso de uso que lo pida. Se puede llegar a él más tarde, porque los registros ya son append-only. |
| **Atribución por cambio sin sesión** (cada evento con su agente y su origen) | Q37 obliga a encontrar todos los eventos de una sesión; sin sesión como entidad, una corrección no sabe hasta dónde llegar. |

## Consecuencias

- ✅ Q37 se cumple por construcción: una corrección es un registro, no una reescritura, y alcanza toda la sesión.
- ✅ Retirar una corrección es inmediato y deja rastro; el modelo admite las dos respuestas posibles a P16 y P17.
- ✅ "Humano" no se puede emitir porque el contrato no tiene esa variante (Q34).
- ✅ Los huecos quedan señalados y sus cambios nunca se atribuyen a un agente (BR-EDGE-005); la Time Machine sabe qué periodos no tienen atribución.
- ✅ La evidencia guardada por evento permite medir la precisión de la detección (SPIKE-GRP-001).
- ⚠️ **Depende de P16 y P17**, abiertas para el PO. **Mitigación**: el modelo cubre ambas respuestas cambiando solo la regla de resolución, no los datos.
- ⚠️ BR-CONS-002 no dice qué pasa al retirar una corrección **si la sesión ya terminó** ("si la sesión sigue presente, vuelve la atribución detectada"). ⚠️ **ASSUMPTION**: se resuelve igual que con la sesión presente. **Pendiente para el PO**.
- ⚠️ En un worktree compartido, los cambios de archivos quedan casi siempre "sin atribuir" en el MVP, porque no hay atribución por archivo (Q7). **Mitigación**: los eventos de Git con evidencia de una sola sesión (por ejemplo, un commit por ascendencia de procesos) sí se atribuyen.
- ⚠️ Resolver el actor exige un cruce entre evento y sesión. **Mitigación**: índices por sesión y por worktree, y la atribución efectiva de cada sesión en memoria del daemon; se mide en INF-GRP-002 dentro del presupuesto de ADR-GRP-011.
- ⚠️ El historial crece sin límite (los eventos no se borran). La retención queda fuera del MVP (ver ADR-GRP-006).

Nota de integración (Time Machine, ADR-TMC-005, TQ-8, aceptado el 2026-10-03): la entidad Sesión guarda, para las sesiones registradas, la identidad no reutilizable del proceso que se registró y su hora de inicio. La Time Machine consume la atribución vigente y el aviso de cambio de atribución de § 6 sin copiarlos.

## Validación

Pruebas con repos y perfiles temporales; las sesiones se simulan con la interfaz de detección de ADR-GRP-012.

1. **Alcance de la corrección** (US-GRP-010): una sesión detectada con commits `c1` y `c2` y una sesión anterior terminada con `c0`; tras corregir, `c1` y `c2` resuelven al agente corregido con origen "registrado" y `c0` no cambia; el worktree sigue con una sola sesión.
2. **Retiro** (P17): retirar la corrección devuelve `c1` y `c2` a la atribución detectada.
3. **Rechazos**: corregir en un worktree sin atribución detectada se rechaza (Q38); una corrección pedida por un agente se rechaza (BR-AUTH-001).
4. **Confirmación** (US-GRP-009): registrar a Claude Code donde ya se detectaba no crea una segunda sesión ni marca compartido; el origen pasa a "registrado" (P16).
5. **Registro como evidencia** (US-GRP-009, BR-VAL-001): con "otro agente: Codex" registrado como única sesión de `feat-login`, un commit sin S2b, S3 ni S4 tiene como actor "otro agente: Codex" (registrado). Tras registrar un segundo agente en el mismo worktree, un commit sin evidencia por evento queda "sin atribuir". Al retirar el segundo registro, vuelve a atribuirse a Codex.
6. **Confirmación sin evidencia por registro** (US-GRP-008, US-GRP-009, BR-EDGE-004): con una sesión de Claude Code detectada y confirmada por registro como única sesión del worktree, una edición del humano sin S2b, S3 ni S4 queda "sin atribuir", también con el origen ya en "registrado" (P16).
7. **Retiro de registro** (US-GRP-009, Q41): retirar el registro de Codex pasa su sesión a "Terminado" con causa "registro retirado" y un commit posterior queda "sin atribuir"; un agente que intenta retirar el registro de otro agente es rechazado y el intento queda en la auditoría (ASSUMPTION).
8. **Compartido** (US-GRP-011): registrar otro agente añade una sesión y el worktree se reporta compartido; un cambio de archivo sin evidencia de una sola sesión queda "sin atribuir".
9. **Huecos** (US-GRP-005): con un agente registrado, matar el daemon, hacer dos commits y relanzarlo; los dos aparecen como eventos de reconciliación "sin atribuir" enlazados a un hueco con su intervalo.
10. **Persistencia** (US-GRP-004): tras reiniciar el daemon, correcciones, sesiones, estados y eventos siguen iguales.
11. **Sin "humano"**: el esquema del contrato no contiene esa variante; una prueba de propiedades sobre secuencias aleatorias de registros comprueba que todo actor resuelto es un agente con origen o "sin atribuir".
12. **Orden**: con la hora del sistema retrasada a mitad de prueba, el orden de los eventos sigue la secuencia.
13. **No repudio (SEC-13)**: un agente simulado que ejecuta `raptor daemon stop` es rechazado y el intento queda en el registro de auditoría; `kill -9` del daemon con una sesión activa deja, al relanzar, un hueco "caída durante sesión activa" visible en los clientes; una parada aceptada guarda el cliente que la pidió.
14. **Auditoría (SEC-03)**: cada comando reservado, aceptado o rechazado, aparece una sola vez en el registro, y el registro no admite modificación ni borrado por el canal. Para los comandos reservados de Guardrails, la entrada lleva además la cadena de ascendencia con ruta e identificador de cada proceso, la terminal, el líder de sesión y los vectores no cubiertos (Enmienda 2026-10-04; ADR-GRD-007, Validación 4).
15. **Repo intacto**: todo lo anterior pasa por el arnés de INF-GRP-001.

## Referencias

- **Reglas**: BR-CONS-002, BR-CONS-003, BR-CONS-004, BR-CONS-005, BR-WF-001, BR-AUTH-001, BR-EDGE-003, BR-EDGE-004, BR-EDGE-005.
- **Historias**: US-GRP-002, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011.
- **Decisiones del context**: Q6, Q7, Q25, Q26, Q31, Q33, Q34, Q35, Q36, Q37, Q38, Q39, Q41; preguntas abiertas P16 y P17.
- **ADRs**: ADR-GRP-005 (único escritor, controles del canal), ADR-GRP-006 (almacén por repo), ADR-GRP-010 (reconciliación), ADR-GRP-012 (señales y evidencia).
- **Enablers**: TS-GRP-001, SPIKE-GRP-001, INF-GRP-001, INF-GRP-002.
- **Features consumidoras**: F-001-03 Time Machine y F-001-04 Guardrails.
- **Seguridad**: `docs/architecture/non-functional.md` (SEC-03, SEC-12, SEC-13).

## Revisión de seguridad (2026-10-03)

Enmienda tras la revisión del security-expert. No cambia el modelo de atribución.

| Hallazgo | Cómo se cubre |
|---|---|
| M6 · Un agente para o mata el daemon para dejar su trabajo sin atribuir | Apartados 1 y 5: el hueco guarda la causa (incluida "caída durante sesión activa") y el cliente que lo provocó; se muestra en los clientes (SEC-13). La parada es comando reservado en ADR-GRP-005 |
| SEC-03 · Registro de auditoría | Apartado 1: registro append-only de comandos reservados en el índice global del perfil, consultable por los clientes |
| M8 / SEC-12 · Texto no confiable en lo que expone el contrato | Apartado 6: nombres declarados, rutas y refs marcados como no confiables; respuestas MCP acotadas |

Validación ampliada: SEC-13 y SEC-03 (puntos 13 y 14).

## Enmienda (2026-10-04, Guardrails)

Aplicada desde la tabla de enmiendas de [non-functional-guardrails.md](../non-functional-guardrails.md) (J10). No cambia el modelo de eventos, sesiones ni atribución. El `status` siguió en `proposed` hasta su aceptación (Rene Bonilla, 2026-10-04).

| Cambio | Dónde | Fuente |
|---|---|---|
| La auditoría append-only incluye los comandos reservados de Guardrails, con la cadena completa de ascendencia, la terminal, el líder de sesión y la aceptación de riesgo por acción | § 1 (registro de auditoría); Validación 14 | ADR-GRD-006 § 4, ADR-GRD-007 § 1 y § 2; D5 |

## Enmienda (2026-10-04, SPIKE-GRP-002)

Derivada de la Enmienda de ADR-GRP-010 con los resultados de [SPIKE-GRP-002](../../requirements/features/motor-local/research/SPIKE-GRP-002-resultados.md), medidos solo en macOS. No cambia el modelo de eventos, sesiones ni atribución: amplía la lista cerrada de causas de hueco para que ADR-GRP-010 y este ADR no se contradigan. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| Causas de hueco nuevas: desbordamiento de la cola del watcher, recreación del stream del watcher y reconciliación periódica, con su intervalo | § 1 (Hueco), § 5 | ADR-GRP-010 § 5 y § 6; decisión del orquestador (2026-10-04), validada por el Arquitecto |

## Enmienda (2026-10-04, ADR-GRD-008)

Aplicada desde la tabla de enmiendas de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md). No cambia el modelo de eventos, sesiones ni atribución. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| La auditoría de los comandos reservados incluye cada intento del factor del SO: acción, resumen del plan, método, resultado (`verified`, `denied`, `cancelled`, `timeout`, `unavailable`, `busy`, `throttled`, `stale`, `precheck-denied`), tipo de autenticación si el SO lo informa y los momentos de petición, resultado, cierre de la ventana y aplicación. Nunca el nonce ni secretos | § 1 (registro de auditoría) | ADR-GRD-008 § 4 |

## Enmienda (2026-10-04, Cockpit)

Aplicada desde DEP-CKP-4 y DEP-CKP-14 (y la parte de rutas de DEP-CKP-1) de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-001](./ADR-CKP-001-prediccion-conflictos-merge-en-seco.md) y [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) (accepted 2026-10-04). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia el modelo de eventos, sesiones ni atribución, ni añade tipos de evento: expone datos derivados de lo que ya se guarda. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| **Última actividad** por worktree, derivada de los eventos y publicada con el estado del worktree | § 6 | DEP-CKP-4; BR-04; BR-CKP-CALC-001 |
| **Última sesión** por worktree, presente o terminada, publicada mientras exista el worktree | § 6 | DEP-CKP-4; Q-CKP-3; BR-CKP-TIME-002 |
| El último estado conocido incluye el **estado en conflicto**; su entrada y su salida se publican como cambios del estado del worktree | § 1, § 6 | DEP-CKP-14; ADR-GRP-010 (Enmienda, Cockpit) |
| El conjunto completo de rutas sin commitear **no se persiste**: el último estado conocido sigue guardando la huella | § 1 | DEP-CKP-1; ADR-CKP-001 § 3 |

- **Última actividad**: la hora de observación (UTC con el desfase local, § 4) del **evento más reciente del worktree**, sin contar los cambios de estado de las sesiones (inactivo o terminado no son actividad). Si ese evento es de reconciliación, se publica con la marca de su hueco, porque la hora real cae dentro del hueco (§ 5). Se deriva en memoria de los eventos del almacén del repo; no es una entidad nueva.
- **Última sesión**: la sesión más reciente del worktree por inicio, presente o terminada, con su agente y origen según la atribución efectiva (§ 2), su inicio, su fin y la causa del fin. Sale de la entidad Sesión (§ 1). La regla de visibilidad de 24 h (BR-CKP-TIME-002) es de la presentación, no del motor.
- **Estado en conflicto**: tipo de operación en curso, oids de `MERGE_HEAD` u `onto`, y rutas sin fusionar con su tope (ADR-GRP-010, Enmienda (2026-10-04, Cockpit)). Forma parte del último estado conocido para que la reconciliación tras un hueco también lo detecte. Rutas y ramas son texto no confiable (SEC-12).
- **Relación commit → evento** para el actor por commit en el grafo (DEP-CKP-2, opcional): **no se decide aquí**; sigue pendiente de motor-local. Sin ella, el Cockpit muestra "sin atribuir" (Q-CKP-4).
- **Forma del contrato** (campos y eventos): **pendiente, dueño: worker del canal (TS-GRP-004)**.

**Validación añadida**: un commit en un worktree actualiza su última actividad; un cambio de sesión a inactivo no la actualiza; tras un hueco, la última actividad lleva la marca del hueco; un worktree cuya sesión terminó publica esa sesión con su fin y su causa mientras existe.

## Enmienda (2026-10-05, MCP)

Decisión del orquestador (2026-10-05), validada por Arquitecto. Origen: DEP-MCP-6 (CTX-MCP-001), ADR-MCP-001 § 4.1 y § 5.

- **Vista MCP de eventos y timeline** para `explain_history`: operación, actor, rama o worktree, oids, rutas con tope, hora, cobertura y nivel del punto. **Nunca** mensajes de commit, contenido ni rutas fuera del repo del llamante. Filtro por id de operación, para que un agente consulte el resultado de una escritura que volvió con `running`. 50 por defecto y 200 como máximo por página, con cursor opaco.
- La consulta del timeline (`timemachine.timeline`, TS-TMC-004) gana la marca MCP **solo con esa vista**; el perfil completo no cambia. La añade la historia dueña (US-MCP-017) al contrato de `crates/api`.
- Los eventos del stream siguen sin llegar a `raptor-mcp` salvo `engine.state` y `daemon.stopping`: `explain_history` es una consulta, no una suscripción.
