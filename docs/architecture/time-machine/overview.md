# Arquitectura — Time Machine (F-001-03)

> Punto de entrada de la arquitectura de la Time Machine: snapshots, undo/redo, restauración y timeline. Es un catálogo de navegación; el detalle vive en cada ADR, enabler y diagrama. Todos los ADRs están en `proposed`: **nada está aprobado hasta que Rene responda las preguntas TQ-n**.
>
> **Restricciones activas**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: rigen como constitución ADR-GRP-001 (Rust; gitoxide para leer, Git CLI para escribir) y ADR-GRP-002 (crates y apps), más las reglas de AGENTS.md (NFR-01, NFR-02). Formalizar con `/aadd-architect --init-constitution`.
>
> **Dependencia de otra rama**: se apoya en la arquitectura de motor-local (ADR-GRP-005..013, TS-GRP-*, INF-GRP-*), aún sin mergear en `docs/arch-motor-local`. Se cita; no se copia ni se edita.

## 1. Propósito y alcance

Guardar un punto recuperable antes de toda operación de GitRaptor y capturar a medida que ocurre el trabajo hecho con Git crudo, para deshacer, rehacer y restaurar sin perder trabajo de nadie (NFR-01, BR-08 a BR-10). La Time Machine solo escribe cuatro cosas: snapshot, undo, redo y restauración (BR-TMC-CONS-004). Fuera de alcance: ejecutar las operaciones de usuario del Cockpit y del MCP (F-001-02, F-001-05; la Time Machine solo las protege), definir políticas e instalar hooks (F-001-04), deshacer en el remoto y la UI del timeline.

## 2. Decisiones (ADRs)

| ID | Título | Decisión (1 línea) | Status |
|----|--------|--------------------|--------|
| [ADR-TMC-001](../decisions/ADR-TMC-001-almacen-snapshots-perfil.md) | Almacén de snapshots | Repo Git bare privado por repo en el perfil, con objetos propios (siembra por enlace duro o copia, más anclaje incremental), contenido en bruto sin filtros, exclusiones declaradas, almacén tratado como entrada no confiable; nunca en el repo del usuario | proposed |
| [ADR-TMC-002](../decisions/ADR-TMC-002-escritor-time-machine.md) | Escritor de la Time Machine | Escrituras internas en el componente `timemachine` del daemon, con capa propia en `crates/git` sin hooks, filtros ni red (SEC-TMC-02); aplicación con precondiciones, snapshot previo, `index.lock` propio, refs con valor esperado e intercambio atómico por archivo; las operaciones de usuario las ejecuta el daemon fuera de esa capa | proposed |
| [ADR-TMC-003](../decisions/ADR-TMC-003-oplog-diario-recuperacion.md) | Oplog, diario y recuperación | SQLite propio por repo, solo por anexión y encadenado por hash; solicitante congelado; al arrancar se descarta, aborta o marca como interrumpido; solo se libera el `index.lock` propio | proposed |
| [ADR-TMC-004](../decisions/ADR-TMC-004-cobertura-dos-niveles.md) | Cobertura en dos niveles | La operación protegida es el único camino de escritura; captura por observación sobre los eventos del motor, fuera de su presupuesto, con coalescencia y cuotas; previo vía hook como contrato para Guardrails | proposed |
| [ADR-TMC-005](../decisions/ADR-TMC-005-solicitante-permisos-solape.md) | Solicitante, permisos y solape | Solicitante por ascendencia endurecida en el daemon (agente X o sin atribuir); reto ligado al plan; MCP sin atribuir rechazado de entrada; Guardrails solo deniega; solape por archivo y ref; riesgo residual aceptado | proposed |
| [ADR-TMC-006](../decisions/ADR-TMC-006-presupuesto-rendimiento-snapshot.md) | Presupuesto del snapshot | p95 < 200 ms del snapshot previo con almacén sembrado, con 1 y con 10 worktrees activos; etapas 180 ms + margen 20 ms; repo mediano fijado por SPIKE-TMC-001; gate en el banco de INF-GRP-002 | proposed |
| [ADR-TMC-007](../decisions/ADR-TMC-007-retencion-purga-segura.md) | Retención y purga | `timeMachine.retentionDays` (perfil y local, 30); protección del previo a la última destructiva por worktree; purga en dos fases con aviso; solo se borran refs del almacén; `forget` fuera del diseño (TQ-17) | proposed |

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

## Preguntas para Rene

> Ningún ADR pasa a `accepted` sin respuesta. Fusionadas: las cuotas de disco entran en TQ-5; la ventana de reemplazo de ADR-TMC-002 deja de ser pregunta porque se adopta SEC-TMC-11; NFR-07 queda en TQ-13.

| ID | Pregunta | Opciones | Recomendación | Afecta |
|----|----------|----------|---------------|--------|
| TQ-1 | ¿Dónde viven los snapshots? | (a) refs ocultas en el repo; (b) almacén en el perfil con `alternates`; (c) almacén autocontenido en el perfil | **(c)**: es la única que cumple las cuatro garantías de D-TMC-11; (a) la expone `push --mirror`, (b) la rompe `gc --prune=now`. Se acepta el disco que mida el spike | ADR-TMC-001 |
| TQ-2 | ¿Dónde vive el escritor interno? | (a) módulo en `crates/core` + capa separada en `crates/git`; (b) crate `crates/timemachine` (enmienda ADR-GRP-002) | **(a)**: ADR-GRP-002 ya asigna oplog/snapshots a `core` y la escritura a `crates/git`; basta una comprobación estática | ADR-TMC-002, ADR-GRP-009 (nota) |
| TQ-3 | ¿Quién ejecuta las operaciones de usuario del Cockpit y del MCP? | (a) el ejecutor de operaciones del daemon (F-001-02/05), fuera de la capa de la Time Machine, con hooks del usuario, tras la operación protegida; (b) el cliente, con un token de guarda | **(a)**: un solo proceso escritor, el solicitante resuelto donde exige ADR-GRP-005 y la Time Machine limitada a sus cuatro escrituras (BR-TMC-CONS-004) | ADR-TMC-002 § 5, ADR-TMC-004, F-001-02/05 |
| TQ-4 | Si el spike falla los 200 ms, ¿se escribe el almacén con gitoxide? | (a) preaprobar como excepción acotada al almacén; (b) solo Git CLI; (c) decidir al cerrar el spike | **(a)**: la razón de ADR-GRP-001 (fidelidad con hooks y config del usuario) no aplica a un almacén privado | ADR-TMC-006, ADR-GRP-001 |
| TQ-5 | Límites de tamaño y disco | (a) sin límites; (b) 50 MB por archivo en la captura por observación + cuota de 20 GB por repo + espacio libre mínimo de máx(5 GB, 5 %), ajustados por el spike; (c) solo cuota configurable | **(b)**: un archivo grande o una ráfaga llenarían el disco; el snapshot previo nunca tiene límite y tiene reserva propia | ADR-TMC-001, 004, 007; SEC-TMC-12 |
| TQ-6 | Granularidad del solape | (a) archivo y ref; (b) fragmento con fusión de tres vías | **(a)**: solo puede detener de más, nunca sobrescribir | ADR-TMC-005 § 5 |
| TQ-7 | ¿"Sin atribuir" por MCP? | (a) rechazo incondicional y previo; (b) como la CLI sin confirmación | **(a)**: un agente no identificado no puede probar que el trabajo es suyo; más estricto que BR-TMC-AUTH-001, no lo contradice | ADR-TMC-005 § 1 |
| TQ-8 | ¿Agentes registrados como solicitantes? | (a) el registro guarda la identidad del proceso; (b) solo Claude Code detectado | **(a)**: sin ella falla el escenario 5 de US-TMC-013 | ADR-TMC-005, ADR-GRP-012/013 |
| TQ-9 | ¿Undos seguidos? | (a) pila por worktree; (b) un undo tras un undo lo revierte | **(a)**: lo que espera quien viene de un editor; decisión de producto | ADR-TMC-003 § 4, US-TMC-002 |
| TQ-10 | ¿Fallo detectado a mitad de un undo o una restauración? | (a) detener, marcar como interrumpida y ofrecer undo; (b) rollback automático | **(a)**: un único camino de recuperación, sin escrituras extra cuando el entorno falla | ADR-TMC-002 § 3 |
| TQ-11 | ¿Aviso antes de purgar? | (a) mostrado en CLI/TUI + 24 h; (b) 24 h sin exigir que se vea; (c) inmediato | **(a)**: "avisar antes" solo se cumple si alguien lo vio | ADR-TMC-007 § 4 |
| TQ-12 | Notas en ADRs de motor-local | Aprobar o no: ADR-GRP-009 (segunda allowlist, de escritura, solo para la Time Machine), 006 (`tm/<id-repo>/` con contenido del usuario), 007 (sección `timeMachine`), 005 (comandos y reutilización de § 6), 012/013 (TQ-8), 011/INF-GRP-002 (gate con la Time Machine activa), INF-GRP-001 (canario reutilizado) | **Aprobar todas**: no cambian ninguna decisión; aplicarlas en `docs/arch-motor-local` o tras el merge. ADR-GRP-002 no cambia | ADR-GRP-005..013 |
| TQ-13 | NFR-07 y ADR-GRP-001 piden respetar la configuración y los hooks del usuario | (a) las escrituras internas (almacén, undo, redo, restauración) no usan hooks, filtros, firma ni la configuración de sistema y global del usuario (SEC-TMC-02); las operaciones de usuario sí la respetan; (b) las internas también la usan; (c) uso opcional en las internas por configuración | **(a)**: una restauración no debe depender de código o configuración del repo y del usuario (hostil o que se cuelga) ni provocar recursión con los hooks de Guardrails; refina NFR-07 solo para las escrituras internas | ADR-TMC-002 § 2, NFR-TMC-12, SEC-TMC-02, NFR-07 |
| TQ-14 | ¿Cómo se confirma tocar trabajo ajeno? **Cambia D-TMC-23 en Windows (decisión de producto)** | (a) en el MVP, ascendencia + multiplexor + reto ligado al plan, con riesgo residual aceptado en ADR-TMC-005 y sin confirmación en Windows hasta tener una prueba (excepción a NFR-TMC-13); (b) presencia verificada por el SO (Touch ID, Windows Hello, polkit) | **(a) ahora y (b) en la Fase 2**: (a) cubre al agente confundido y el snapshot previo compensa el residuo; en Windows es preferible no confirmar a confirmar sin prueba | ADR-TMC-005 § 3, SEC-TMC-03, D-TMC-23, BR-TMC-AUTH-001, NFR-TMC-13 |
| TQ-15 | ¿Repos anidados sin seguimiento? | (a) excluir y declarar; (b) capturarlos como contenido | **(a)**: son otros repos, y escribirlos o borrarlos al restaurar es peligroso | ADR-TMC-001 § 2, SEC-TMC-04 |
| TQ-16 | ¿Credenciales sin ignorar? **Cambia D-TMC-16 (decisión de producto)** | (a) lista cerrada de credenciales excluida por defecto, con opción en el perfil; (b) mantener D-TMC-16 (se captura todo lo no ignorado) | **(a)**: evita copiar secretos al perfil sin perder código. Requiere que el PO actualice D-TMC-16 | ADR-TMC-001; SEC-TMC-06; D-TMC-16, BR-TMC-CONS-002 |
| TQ-17 | ¿Borrar ya contenido capturado (`raptor tm forget`)? **Capacidad nueva de producto, sin US** | (a) aplazarla a una US futura; en el MVP solo la lista de exclusión de TQ-16; (b) incluirla en el MVP reescribiendo los snapshots sin esa ruta (sin perder el resto), respetando las capturas en curso y el snapshot protegido, con una US que pide el PO | **(a)**: tal como se propuso, chocaba con la retención y la protección del último punto y ampliaba lo que la Time Machine puede escribir; (b) es viable pero es trabajo de producto y de diseño propio, no un detalle de seguridad | ADR-TMC-007 § 5, SEC-TMC-06; D-TMC-15, BR-TMC-TIME-001, BR-TMC-CONS-004 |

## 7. Pendientes de integración y notas para el PO

Al mergear con `docs/arch-motor-local`, sin tocar ahora esos archivos:

1. `docs/architecture/decisions/index.md`: añadir ADR-TMC-001..007 al outline y al grafo.
2. `docs/architecture/architecture-overview.md`: enlazar este overview, los diagramas `*-tmc-*` y los enablers TMC.
3. `docs/architecture/non-functional.md`: enlazar [non-functional.md](./non-functional.md) y referenciar SEC-TMC-01..15 junto a SEC-01..14.
4. `docs/architecture/diagrams/c4-containers.md`: anotar que el daemon aloja la Time Machine y el ejecutor de operaciones de usuario, y que el perfil contiene el almacén de snapshots.
5. Las notas de TQ-12 en los ADR-GRP afectados.
6. `docs/ARTIFACTS.md` lo regenera `/aadd-index` (no se edita a mano).

**Notas para el PO** (no se edita el requerimiento desde la arquitectura):

- **BR-TMC-CONS-004**: la recuperación al arrancar borra un `index.lock` creado por la propia Time Machine, sin que nadie lo pida (ADR-TMC-003 § 6.4). Es una excepción explícita: libera un lock propio y no toca contenido. Conviene recogerla en la regla.
- **D-TMC-16**: si TQ-16 = (a), la lista de credenciales pasa a tratarse como ignorada; hay que actualizar la decisión y BR-TMC-CONS-002.
- **NFR-07**: si TQ-13 = (a), dejar escrito que las escrituras internas de la Time Machine no usan hooks, filtros, firma ni la configuración de sistema y global del usuario.
- **D-TMC-23 y BR-TMC-AUTH-001**: si TQ-14 = (a), en Windows un solicitante "sin atribuir" no puede tocar trabajo de un agente (no hay confirmación hasta tener una prueba equivalente). Cambia la decisión en ese SO y es una excepción a NFR-TMC-13.
- **`forget` (TQ-17)**: capacidad nueva sin US. Si se quiere, el PO debe escribir su historia y conciliarla con D-TMC-15, BR-TMC-TIME-001 y BR-TMC-CONS-004.

## 8. Riesgos abiertos

- **R2** (aceptado): lo editado entre la última captura y una operación destructiva de Git crudo sin hooks puede perderse (ventana de ADR-TMC-004 § 2).
- **Evasión del árbol de procesos** (aceptado en ADR-TMC-005): un proceso del mismo usuario que se desacopla (doble fork, `setsid`, `launchctl`, `systemd-run`, `osascript`) puede pasar por "sin atribuir" y confirmar. Mitigado con el snapshot previo de cada operación; la auditoría del oplog es manipulable por el mismo usuario (SEC-TMC-09).
- **Disco del almacén** (TQ-1, TQ-5): pendiente de la medición de SPIKE-TMC-001.
- **P17 de motor-local**: bloquea US-TMC-011 (D-TMC-22).
