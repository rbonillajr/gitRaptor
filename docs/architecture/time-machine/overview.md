# Arquitectura — Time Machine (F-001-03)

> Punto de entrada de la arquitectura de la Time Machine: snapshots, undo/redo, restauración y timeline. Es un catálogo de navegación; el detalle vive en cada ADR, enabler y diagrama. Rene Bonilla aceptó el 2026-10-03 todas las recomendaciones TQ-1 a TQ-17: los ADRs TMC están en `accepted`. Quedan pendientes las notas a motor-local (§ 7) y las actualizaciones del PO.
>
> **Restricciones activas**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: rigen como constitución ADR-GRP-001 (Rust; gitoxide para leer, Git CLI para escribir) y ADR-GRP-002 (crates y apps), más las reglas de AGENTS.md (NFR-01, NFR-02). Formalizar con `/aadd-architect --init-constitution`.
>
> **Dependencia de otra rama**: se apoya en la arquitectura de motor-local (ADR-GRP-005..013, TS-GRP-*, INF-GRP-*), aún sin mergear en `docs/arch-motor-local`. Se cita; no se copia ni se edita.

## 1. Propósito y alcance

Guardar un punto recuperable antes de toda operación de GitRaptor y capturar a medida que ocurre el trabajo hecho con Git crudo, para deshacer, rehacer y restaurar sin perder trabajo de nadie (NFR-01, BR-08 a BR-10). La Time Machine solo escribe cuatro cosas: snapshot, undo, redo y restauración (BR-TMC-CONS-004). Fuera de alcance: ejecutar las operaciones de usuario del Cockpit y del MCP (F-001-02, F-001-05; la Time Machine solo las protege), definir políticas e instalar hooks (F-001-04), deshacer en el remoto y la UI del timeline.

## 2. Decisiones (ADRs)

| ID | Título | Decisión (1 línea) | Status |
|----|--------|--------------------|--------|
| [ADR-TMC-001](../decisions/ADR-TMC-001-almacen-snapshots-perfil.md) | Almacén de snapshots | Repo Git bare privado por repo en el perfil, con objetos propios (siembra por clon con copia en escritura o copia, nunca por enlace duro, más anclaje incremental; escritura con gitoxide), contenido en bruto sin filtros, exclusiones declaradas, almacén tratado como entrada no confiable; nunca en el repo del usuario | accepted |
| [ADR-TMC-002](../decisions/ADR-TMC-002-escritor-time-machine.md) | Escritor de la Time Machine | Escrituras internas en el componente `timemachine` del daemon, con capa propia en `crates/git` sin hooks, filtros ni red (SEC-TMC-02); aplicación con precondiciones, snapshot previo, `index.lock` propio, refs con valor esperado e intercambio atómico por archivo; las operaciones de usuario las ejecuta el daemon fuera de esa capa | accepted |
| [ADR-TMC-003](../decisions/ADR-TMC-003-oplog-diario-recuperacion.md) | Oplog, diario y recuperación | SQLite propio por repo, solo por anexión y encadenado por hash; solicitante congelado; al arrancar se descarta, aborta o marca como interrumpido; solo se libera el `index.lock` propio | accepted |
| [ADR-TMC-004](../decisions/ADR-TMC-004-cobertura-dos-niveles.md) | Cobertura en dos niveles | La operación protegida es el único camino de escritura; captura por observación sobre los eventos del motor, fuera de su presupuesto, con coalescencia y cuotas; previo vía hook como contrato para Guardrails | accepted |
| [ADR-TMC-005](../decisions/ADR-TMC-005-solicitante-permisos-solape.md) | Solicitante, permisos y solape | Solicitante por ascendencia endurecida en el daemon (agente X o sin atribuir); reto ligado al plan; MCP sin atribuir rechazado de entrada; Guardrails solo deniega; solape por archivo y ref; riesgo residual aceptado | accepted |
| [ADR-TMC-006](../decisions/ADR-TMC-006-presupuesto-rendimiento-snapshot.md) | Presupuesto del snapshot | p95 < 200 ms del snapshot previo con almacén sembrado, con 1 y con 10 worktrees activos; etapas 180 ms + margen 20 ms, con cifras medidas en macOS; repo mediano = perfil `M` (SPIKE-TMC-001); escalones 2 y 3 obligatorios; gate en el banco de INF-GRP-002 | accepted |
| [ADR-TMC-007](../decisions/ADR-TMC-007-retencion-purga-segura.md) | Retención y purga | `timeMachine.retentionDays` (perfil y local, 30); protección del previo a la última destructiva por worktree; purga en dos fases con aviso; solo se borran refs del almacén; `forget` fuera del diseño (TQ-17) | accepted |

### Grafo de dependencias

| ADR | Depende de | Lo consumen |
|-----|-----------|-------------|
| ADR-TMC-001 | ADR-GRP-006, 009, 010 | TMC-002, 003, 004, 006, 007 |
| ADR-TMC-002 | ADR-GRP-001, 002, 005, 009; TMC-001 | TMC-003, 004, 005 |
| ADR-TMC-003 | ADR-GRP-006, 013; TMC-001, 002 | TMC-004, 005, 007 |
| ADR-TMC-004 | ADR-GRP-005, 010, 011, 012; TMC-001, 002, 003 | TMC-006 |
| ADR-TMC-005 | ADR-GRP-005 § 6, 007, 012, 013; TMC-002, 003 | — |
| ADR-TMC-006 | ADR-GRP-001, 011; TMC-001, 004 | — (lo valida SPIKE-TMC-001) |
| ADR-TMC-007 | ADR-GRP-007, 008; TMC-001, 003 | — |

## 3. Vista del sistema y flujos críticos

- **Componentes**: [c4-tmc-components.md](../diagrams/c4-tmc-components.md). Las vistas de contexto y contenedores son las de motor-local (otra rama); la Time Machine no añade contenedores, vive dentro del daemon junto al ejecutor de operaciones de usuario.
- **Snapshot previo garantizado**: [seq-tmc-snapshot-previo.md](../diagrams/seq-tmc-snapshot-previo.md).
- **Undo con solicitante, confirmación y solape**: [seq-tmc-undo-solicitante.md](../diagrams/seq-tmc-undo-solicitante.md).
- **Recuperación tras caos**: [seq-tmc-recuperacion.md](../diagrams/seq-tmc-recuperacion.md).

## 4. Atributos de calidad

[non-functional.md](./non-functional.md). Los tres que dirigen el diseño: **NFR-TMC-02** (garantías de D-TMC-11, que deciden el almacén), **NFR-TMC-06** (caos, que decide el diario) y **NFR-TMC-04** (200 ms, que decide la captura incremental y el camino rápido). "Security NFRs" SEC-TMC-01..15, con la revisión del `security-expert` incorporada.

## 5. Enablers

[technical-stories.md](../../requirements/features/time-machine/technical-stories.md): 6 enablers para 21 historias. SPIKE-TMC-001 (repo mediano y viabilidad), TS-TMC-001 (almacén y captura), TS-TMC-002 (oplog y diario), TS-TMC-003 (escritura y aplicador), TS-TMC-004 (operación protegida y solicitante), INF-TMC-001 (arnés de caos, garantías y casos hostiles).

## Mapa US → ADR → enabler

| Historia | ADRs | Enablers | Trabajo propio (Requisitos Técnicos de la US) |
|----------|------|----------|-----------------------------------------------|
| US-TMC-001 | 001, 003, 004 | TS-TMC-001, 002, 004, INF-TMC-001, SPIKE-TMC-001 | Snapshot previo y registro; el catálogo de operaciones es de F-001-02/05 |
| US-TMC-002 | 002, 003, 005 | TS-TMC-002, 003, 004 | Destino del undo de la última operación |
| US-TMC-003 | 003, 005 | TS-TMC-002, 003, 004 | Redo y su solape (S5) |
| US-TMC-004 | 001, 004 | TS-TMC-001, SPIKE-TMC-001 | Disparadores, cadencia y coalescencia de la captura |
| US-TMC-005 | 004 § 3 | TS-TMC-001, 004 | Comando de snapshot previo para hooks |
| US-TMC-006 | 003, 004 § 4 | TS-TMC-002 | Consulta del timeline con nivel y actor vigente |
| US-TMC-007 | 003 | TS-TMC-002 | Filtros y huecos |
| US-TMC-008 | 003 § 5 | TS-TMC-002 | Atribución vigente frente a solicitante congelado |
| US-TMC-009 | 001, 002, 003, 005 | TS-TMC-001..004 | Alcance de la restauración (D-TMC-20) |
| US-TMC-010 | 003, 005 | TS-TMC-002, 003, 004 | Conjunto por periodo |
| US-TMC-011 | 003, 005 | TS-TMC-002, 003, 004 | Conjunto por agente (bloqueada por P17) |
| US-TMC-012 | 005 § 5 | — | Detección del solape (dueña) |
| US-TMC-013 | 005 § 2-3 | TS-TMC-004 | Regla base y confirmación (dueña) |
| US-TMC-014 | 002 § 4 | TS-TMC-003 | Aviso de ya empujado |
| US-TMC-015 | 002 § 3 | TS-TMC-003 | Precondiciones de Git en curso y locks |
| US-TMC-016 | 007 | TS-TMC-001, 002, INF-TMC-001 | Purga en dos fases (dueña) |
| US-TMC-017 | 007 § 1 | — | Clave `timeMachine.retentionDays` |
| US-TMC-018 | 001 | TS-TMC-001, INF-TMC-001 | Escenarios de garantías |
| US-TMC-019 | 003 § 6 | TS-TMC-002, 003, INF-TMC-001 | Aviso de recuperación al cliente |
| US-TMC-020 | 006 | TS-TMC-001, SPIKE-TMC-001 | Escenario del banco en INF-GRP-002 |
| US-TMC-021 | 005 § 4 | — | Punto de evaluación de políticas |

## 6. Dev Specs

Pendientes. Se generan con `/aadd-devspec <id>` para TS-TMC-001..004 e INF-TMC-001, y para las US que producen código. SPIKE-TMC-001 entrega un Research Brief.

## Preguntas para Rene — resueltas (2026-10-03)

> Resueltas: Rene Bonilla aceptó todas las recomendaciones el 2026-10-03. Fusionadas en su día: las cuotas de disco entran en TQ-5; la ventana de reemplazo de ADR-TMC-002 deja de ser pregunta porque se adopta SEC-TMC-11; NFR-07 queda en TQ-13.

| ID | Pregunta | Opciones | Recomendación | Afecta | Decisión |
|----|----------|----------|---------------|--------|----------|
| TQ-1 | ¿Dónde viven los snapshots? | (a) refs ocultas en el repo; (b) almacén en el perfil con `alternates`; (c) almacén autocontenido en el perfil | **(c)**: es la única que cumple las cuatro garantías de D-TMC-11; (a) la expone `push --mirror`, (b) la rompe `gc --prune=now`. Se acepta el disco que mida el spike | ADR-TMC-001 | Aceptada (Rene, 2026-10-03) |
| TQ-2 | ¿Dónde vive el escritor interno? | (a) módulo en `crates/core` + capa separada en `crates/git`; (b) crate `crates/timemachine` (enmienda ADR-GRP-002) | **(a)**: ADR-GRP-002 ya asigna oplog/snapshots a `core` y la escritura a `crates/git`; basta una comprobación estática | ADR-TMC-002, ADR-GRP-009 (nota) | Aceptada (Rene, 2026-10-03) |
| TQ-3 | ¿Quién ejecuta las operaciones de usuario del Cockpit y del MCP? | (a) el ejecutor de operaciones del daemon (F-001-02/05), fuera de la capa de la Time Machine, con hooks del usuario, tras la operación protegida; (b) el cliente, con un token de guarda | **(a)**: un solo proceso escritor, el solicitante resuelto donde exige ADR-GRP-005 y la Time Machine limitada a sus cuatro escrituras (BR-TMC-CONS-004) | ADR-TMC-002 § 5, ADR-TMC-004, F-001-02/05 | Aceptada (Rene, 2026-10-03) |
| TQ-4 | Si el spike falla los 200 ms, ¿se escribe el almacén con gitoxide? | (a) preaprobar como excepción acotada al almacén; (b) solo Git CLI; (c) decidir al cerrar el spike | **(a)**: la razón de ADR-GRP-001 (fidelidad con hooks y config del usuario) no aplica a un almacén privado | ADR-TMC-006, ADR-GRP-001 | Aceptada (Rene, 2026-10-03) |
| TQ-5 | Límites de tamaño y disco | (a) sin límites; (b) 50 MB por archivo en la captura por observación + cuota de 20 GB por repo + espacio libre mínimo de máx(5 GB, 5 %), ajustados por el spike; (c) solo cuota configurable | **(b)**: un archivo grande o una ráfaga llenarían el disco; el snapshot previo nunca tiene límite y tiene reserva propia | ADR-TMC-001, 004, 007; SEC-TMC-12 | Aceptada (Rene, 2026-10-03) |
| TQ-6 | Granularidad del solape | (a) archivo y ref; (b) fragmento con fusión de tres vías | **(a)**: solo puede detener de más, nunca sobrescribir | ADR-TMC-005 § 5 | Aceptada (Rene, 2026-10-03) |
| TQ-7 | ¿"Sin atribuir" por MCP? | (a) rechazo incondicional y previo; (b) como la CLI sin confirmación | **(a)**: un agente no identificado no puede probar que el trabajo es suyo; más estricto que BR-TMC-AUTH-001, no lo contradice | ADR-TMC-005 § 1 | Aceptada (Rene, 2026-10-03) |
| TQ-8 | ¿Agentes registrados como solicitantes? | (a) el registro guarda la identidad del proceso; (b) solo Claude Code detectado | **(a)**: sin ella falla el escenario 5 de US-TMC-013 | ADR-TMC-005, ADR-GRP-012/013 | Aceptada (Rene, 2026-10-03) |
| TQ-9 | ¿Undos seguidos? | (a) pila por worktree; (b) un undo tras un undo lo revierte | **(a)**: lo que espera quien viene de un editor; decisión de producto | ADR-TMC-003 § 4, US-TMC-002 | Aceptada (Rene, 2026-10-03) |
| TQ-10 | ¿Fallo detectado a mitad de un undo o una restauración? | (a) detener, marcar como interrumpida y ofrecer undo; (b) rollback automático | **(a)**: un único camino de recuperación, sin escrituras extra cuando el entorno falla | ADR-TMC-002 § 3 | Aceptada (Rene, 2026-10-03) |
| TQ-11 | ¿Aviso antes de purgar? | (a) mostrado en CLI/TUI + 24 h desde que se mostró por primera vez (D-TMC-25); (b) 24 h sin exigir que se vea; (c) inmediato | **(a)**: "avisar antes" solo se cumple si alguien lo vio | ADR-TMC-007 § 4 | Aceptada (Rene, 2026-10-03) |
| TQ-12 | Notas en ADRs de motor-local | Aprobar o no: ADR-GRP-009 (segunda allowlist, de escritura, solo para la Time Machine), 006 (`tm/<id-repo>/` con contenido del usuario), 007 (sección `timeMachine`), 005 (comandos y reutilización de § 6), 012/013 (TQ-8), 011/INF-GRP-002 (gate con la Time Machine activa), INF-GRP-001 (canario reutilizado) | **Aprobar todas**: no cambian ninguna decisión; aplicarlas en `docs/arch-motor-local` o tras el merge. ADR-GRP-002 no cambia | ADR-GRP-005..013 | Aceptada (Rene, 2026-10-03) |
| TQ-13 | NFR-07 y ADR-GRP-001 piden respetar la configuración y los hooks del usuario | (a) las escrituras internas (almacén, undo, redo, restauración) no usan hooks, filtros, firma ni la configuración de sistema y global del usuario (SEC-TMC-02); las operaciones de usuario sí la respetan; (b) las internas también la usan; (c) uso opcional en las internas por configuración | **(a)**: una restauración no debe depender de código o configuración del repo y del usuario (hostil o que se cuelga) ni provocar recursión con los hooks de Guardrails; refina NFR-07 solo para las escrituras internas | ADR-TMC-002 § 2, NFR-TMC-12, SEC-TMC-02, NFR-07 | Aceptada (Rene, 2026-10-03) |
| TQ-14 | ¿Cómo se confirma tocar trabajo ajeno? **Cambia D-TMC-23 en Windows (decisión de producto)** | (a) en el MVP, ascendencia + multiplexor + reto ligado al plan, con riesgo residual aceptado en ADR-TMC-005 y sin confirmación en Windows hasta tener una prueba (excepción a NFR-TMC-13); (b) presencia verificada por el SO (Touch ID, Windows Hello, polkit) | **(a) ahora y (b) en la Fase 2**: (a) cubre al agente confundido y el snapshot previo compensa el residuo; en Windows es preferible no confirmar a confirmar sin prueba | ADR-TMC-005 § 3, SEC-TMC-03, D-TMC-23, BR-TMC-AUTH-001, NFR-TMC-13 | Aceptada (Rene, 2026-10-03) |
| TQ-15 | ¿Repos anidados sin seguimiento? | (a) excluir y declarar; (b) capturarlos como contenido | **(a)**: son otros repos, y escribirlos o borrarlos al restaurar es peligroso | ADR-TMC-001 § 2, SEC-TMC-04 | Aceptada (Rene, 2026-10-03) |
| TQ-16 | ¿Credenciales sin ignorar? **Cambia D-TMC-16 (decisión de producto)** | (a) lista cerrada de credenciales excluida por defecto, con opción en el perfil; (b) mantener D-TMC-16 (se captura todo lo no ignorado) | **(a)**: evita copiar secretos al perfil sin perder código. Requiere que el PO actualice D-TMC-16 | ADR-TMC-001; SEC-TMC-06; D-TMC-16, BR-TMC-CONS-002 | Aceptada (Rene, 2026-10-03) |
| TQ-17 | ¿Borrar ya contenido capturado (`raptor tm forget`)? **Capacidad nueva de producto, sin US** | (a) aplazarla a una US futura; en el MVP solo la lista de exclusión de TQ-16; (b) incluirla en el MVP reescribiendo los snapshots sin esa ruta (sin perder el resto), respetando las capturas en curso y el snapshot protegido, con una US que pide el PO | **(a)**: tal como se propuso, chocaba con la retención y la protección del último punto y ampliaba lo que la Time Machine puede escribir; (b) es viable pero es trabajo de producto y de diseño propio, no un detalle de seguridad | ADR-TMC-007 § 5, SEC-TMC-06; D-TMC-15, BR-TMC-TIME-001, BR-TMC-CONS-004 | Aceptada (Rene, 2026-10-03) |

## 7. Integración en los catálogos compartidos y notas para el PO

La arquitectura de motor-local ya está en `main` (PR #8) y los puntos 1 a 5 están integrados. Solo quedan las notas para el PO.

1. ~~`docs/architecture/decisions/index.md`: añadir ADR-TMC-001..007 al outline y al grafo~~: **hecho** ([índice de ADRs](../decisions/index.md), Outline, grafo y subsección "Feature time-machine").
2. ~~`docs/architecture/architecture-overview.md`: enlazar este overview, los diagramas `*-tmc-*` y los enablers TMC~~: **hecho** ([architecture overview](../architecture-overview.md), §§ 1, 4, 5 y 7).
3. ~~`docs/architecture/non-functional.md`: enlazar [non-functional.md](./non-functional.md) y referenciar SEC-TMC-01..15 junto a SEC-01..14~~: **hecho** ([NFR del motor](../non-functional.md), nota de cabecera).
4. ~~`docs/architecture/diagrams/c4-containers.md`: anotar que el daemon aloja la Time Machine y el ejecutor de operaciones de usuario, y que el perfil contiene el almacén de snapshots~~: **hecho** ([C4-L2](../diagrams/c4-containers.md), nota bajo el diagrama).
5. ~~Las notas de TQ-12~~: **aplicadas** en motor-local (PR #8); ver § 7.1.
6. `docs/ARTIFACTS.md` lo regenera `/aadd-index` (no se edita a mano).

### 7.1 Notas de TQ-12 en los artefactos de motor-local — aplicadas (PR #8)

**Estado**: aplicadas en `main` con el PR #8, al final de la sección "Consecuencias" de cada archivo indicado, sin cambiar ninguna decisión de esos ADRs ni su status. El texto se conserva aquí como registro de lo aprobado.

- **ADR-GRP-005**: "Nota de integración (Time Machine, ADR-TMC-002, ADR-TMC-004 y ADR-TMC-005, aceptados el 2026-10-03): el canal expone además la operación protegida de la Time Machine (intención, snapshot previo y registro) y sus comandos de snapshot, undo, redo, restauración y timeline. La confirmación para deshacer trabajo ajeno reutiliza los controles del daemon de § 6 (identificador no reutilizable, ascendencia, terminal de control y líder de sesión), ampliados con la hora de inicio de cada antecesor, la contaminación por multiplexor y un reto de un solo uso ligado al plan (SEC-TMC-03). Un undo no es un comando reservado: un agente puede deshacer su propio trabajo."
- **ADR-GRP-006**: "Nota de integración (Time Machine, ADR-TMC-001 y ADR-TMC-003, aceptados el 2026-10-03): la carpeta de datos del perfil incluye `tm/<id-repo>/`, con el almacén de snapshots (repo Git bare) y el oplog de la Time Machine (SQLite). A diferencia del almacén del motor, contiene contenido de archivos del usuario: carpeta 0700, archivos 0600, excluida de las copias de seguridad del SO y escrita solo por el daemon (SEC-TMC-01, SEC-TMC-06). La regla de § 4 'solo metadatos' aplica al almacén del motor."
- **ADR-GRP-007**: "Nota de integración (Time Machine, ADR-TMC-007 y SEC-TMC-06/12, aceptados el 2026-10-03): nueva sección `timeMachine`, con tipos en `crates/policy`. Claves: `timeMachine.retentionDays` (entero de 1 a 3650; niveles perfil y local; por defecto 30); `timeMachine.storeQuotaGB` (entero ≥ 1; solo perfil; por defecto 20); `timeMachine.includeCredentialFiles` (booleano; solo perfil; por defecto false). En el nivel de equipo, las tres se ignoran con diagnóstico."
- **ADR-GRP-009**: "Nota de integración (Time Machine, ADR-TMC-002, aceptado el 2026-10-03): la frontera de solo lectura de este ADR es la del motor. `crates/git` aloja además una segunda lista cerrada, de escritura, que solo usa el módulo `timemachine` de `crates/core` (SEC-TMC-02, SEC-TMC-14). La comprobación estática de la Validación 5 se amplía: ni el observador del motor ni el ejecutor de operaciones de usuario pueden importar la capa de escritura de la Time Machine."
- **ADR-GRP-011**: "Nota de integración (Time Machine, ADR-TMC-004 y ADR-TMC-006, aceptados el 2026-10-03): la captura por observación de la Time Machine consume eventos ya publicados y no forma parte de este presupuesto. El gate del motor (p95 ≤ 300 ms) se ejecuta también con la Time Machine activa."
- **ADR-GRP-012**: "Nota de integración (Time Machine, ADR-TMC-005, TQ-8, aceptado el 2026-10-03): el registro explícito de un agente guarda la identidad no reutilizable del proceso que se registra (pidfd, audit token o handle, con su hora de inicio), para que la ascendencia reconozca a ese agente como solicitante de un undo, un redo o una restauración."
- **ADR-GRP-013**: "Nota de integración (Time Machine, ADR-TMC-005, TQ-8, aceptado el 2026-10-03): la entidad Sesión guarda, para las sesiones registradas, la identidad no reutilizable del proceso que se registró y su hora de inicio. La Time Machine consume la atribución vigente y el aviso de cambio de atribución de § 6 sin copiarlos."
- **INF-GRP-001**: "Nota de integración (Time Machine, INF-TMC-001): la huella 'repo intacto' y el repo canario se reutilizan desde INF-TMC-001. El canario se amplía con los casos de SEC-TMC-02: `core.fsmonitor`, `core.worktree` hacia fuera del repo, `includeIf` hostil, `filter.*`, `commit.gpgSign` global e `init.templateDir` con hooks."
- **INF-GRP-002**: "Nota de integración (Time Machine, ADR-TMC-006 y US-TMC-020): el banco añade el escenario 'operación protegida con trabajo sin commitear' sobre el repo de referencia de SPIKE-TMC-001, con 1 y con 10 worktrees activos, con gate de p95 < 200 ms del snapshot previo y aviso por etapa. El gate del motor se ejecuta con la Time Machine activa."

### 7.2 Notas de SPIKE-TMC-001 para motor-local — pendientes de integración

**Estado**: **pendientes**. Salen de las enmiendas de SPIKE-TMC-001 (2026-10-04) a los ADR de la Time Machine. Esta rama solo toca docs de time-machine, así que se aplican en una rama de motor-local. Ninguna cambia una decisión de motor-local salvo la que se indica.

- **ADR-GRP-001**: "Nota de integración (Time Machine, ADR-TMC-006 § 5, enmienda del 2026-10-04): la excepción preaprobada en TQ-4 → a queda **activada**. El almacén de snapshots de la Time Machine se escribe con gitoxide en el proceso, en un submódulo de la capa de escritura de la Time Machine (ADR-TMC-002 § 1). El repo del usuario sigue escribiéndose solo con Git CLI."
- **ADR-GRP-009 § 4** (E6): resolver el binario de Git en macOS sin el shim `/usr/bin/git` (unos 17 ms extra por proceso) y sin ejecutar `xcrun`, leyendo `/var/db/xcode_select_link`. Afecta al motor, al ejecutor y a la Time Machine, que usan la misma resolución.
- **ADR-GRP-009 § 3** (E9): valorar `core.untrackedCache=false` en lugar de `keep`. `keep` lee una untracked cache que un agente puede falsificar. Hay que comprobar que gix la ignora en la versión fijada. Es un cambio de opción, no de decisión.
- **ADR-GRP-010 / TS-GRP-002 / TS-GRP-003** (E2): exponer por worktree "rutas cambiadas desde la marca X", con un indicador de continuidad que se da por roto ante un hueco, el modo degradado, un reinicio, un desbordamiento o un cambio en las reglas de ignore. La Time Machine lo consume para el escalón 2 de ADR-TMC-006 § 5.
- **INF-GRP-002** (E4): el banco usa el generador determinista de SPIKE-TMC-001 (`spikes/snapshot-overhead/`) con el perfil `M` como repo de referencia y el `L` para el caso fuera de referencia.

**Notas para el PO** (no se edita el requerimiento desde la arquitectura; el PO las aplica en paralelo en `business-rules.md` y `context.md`):

- **BR-TMC-CONS-004**: la recuperación al arrancar borra un `index.lock` creado por la propia Time Machine, sin que nadie lo pida (ADR-TMC-003 § 6.4). Es una excepción explícita: libera un lock propio y no toca contenido. Conviene recogerla en la regla.
- **D-TMC-16** (TQ-16 → a): la lista de credenciales pasa a tratarse como ignorada; hay que actualizar la decisión y BR-TMC-CONS-002.
- **NFR-07** (TQ-13 → a): dejar escrito que las escrituras internas de la Time Machine no usan hooks, filtros, firma ni la configuración de sistema y global del usuario.
- **D-TMC-23 y BR-TMC-AUTH-001** (TQ-14 → a): en Windows, un solicitante "sin atribuir" no puede tocar trabajo de un agente: no hay confirmación hasta tener una prueba equivalente. Cambia la decisión en ese SO y es una excepción a NFR-TMC-13.
- **`forget`** (TQ-17 → a): aplazado a una US futura. Cuando se quiera, el PO escribe su historia y la concilia con D-TMC-15, BR-TMC-TIME-001 y BR-TMC-CONS-004.

## 8. Riesgos abiertos

- **R2** (aceptado): lo editado entre la última captura y una operación destructiva de Git crudo sin hooks puede perderse (ventana de ADR-TMC-004 § 2).
- **Evasión del árbol de procesos** (aceptado en ADR-TMC-005): un proceso del mismo usuario que se desacopla (doble fork, `setsid`, `launchctl`, `systemd-run`, `osascript`) puede pasar por "sin atribuir" y confirmar. Mitigado con el snapshot previo de cada operación; la auditoría del oplog es manipulable por el mismo usuario (SEC-TMC-09).
- **Disco del almacén** (TQ-1, TQ-5): medido por SPIKE-TMC-001 en macOS y aceptable (un mes de capturas, unos 200 MiB tras consolidar). El riesgo son los archivos grandes de los previos garantizados frente a las cuotas de SEC-TMC-12. Linux y Windows sin medir.
- **Continuidad del motor** (SPIKE-TMC-001, E2): el p95 de NFR-04 depende de que TS-GRP-002/003 expongan las rutas cambiadas desde una marca (§ 7.2). Sin eso, el previo hace detección completa y no cumple el presupuesto.
- **P17 de motor-local**: bloquea US-TMC-011 (D-TMC-22).
