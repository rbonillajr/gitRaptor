---
mode: draft
status: expanded
generated: 2026-10-03
updated: 2026-10-03
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 26
expanded: 22
approved: 0
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016]
---

# Architecture Overview — Motor local (F-001-01)

> Documento de navegación: enlaza los artefactos y no duplica su contenido. Rene Bonilla aceptó el 2026-10-03 todas las recomendaciones y las preguntas de producto PQ-1 a PQ-9; los ADR 005 a 013 están expandidos (`status: proposed`).
>
> **Revisión de seguridad (2026-10-03)**: aprobada con condiciones por el `security-expert`, sin hallazgos Critical. Tras las enmiendas a ADR-GRP-005, 006, 007, 009, 010, 012 y 013, los cuatro High (H1-H4) quedan cubiertos en texto; ADR-GRP-005 y ADR-GRP-009 pasan a `accepted` cuando INF-GRP-001 tenga el repo canario y la auditoría dinámica de `exec`. Detalle, SEC-01 a SEC-14 y gate en [non-functional.md](./non-functional.md#gate-de-seguridad).
>
> **Restricciones activas**: no hay `architecture-constitution.md` en la cascada. Rigen ADR-GRP-001 (stack) y ADR-GRP-002 (monorepo y crates). ⚠️ **ASSUMPTION**: se tratan como constitución hasta que se cree una formal con `/aadd-architect --init-constitution`.

## 1. Propósito y alcance

El motor local es la única fuente de verdad de GitRaptor (BRD § 11). Observa los repos que añade el desarrollador, sus worktrees, los eventos de Git y las sesiones de agentes, y lo expone a la CLI/TUI y al MCP.

Garantías que fijan el diseño:

- **Solo observa**. No escribe nada en el repo observado, ni siquiera de forma transitoria, y nunca ejecuta programas configurados por el usuario (Q21, [ADR-GRP-009](./decisions/ADR-GRP-009-frontera-solo-lectura-git.md)). Fuera del repo solo escribe su perfil (Q17), con una excepción acotada: el registro del autoarranque, que hace el instalador o `raptor daemon enable` (PQ-1, [ADR-GRP-005](./decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md)).
- **Observa sin huecos** aunque no haya ninguna superficie abierta (BR-CONS-005). Lo que no ve queda "sin atribuir" con marca de hueco.
- **Nunca emite "humano"**: el actor es un agente con su origen o "sin atribuir" (Q34).
- Funciona igual en Windows, macOS y Linux.

Diagramas: [contexto C4-L1](./diagrams/c4-context.md) y [contenedores C4-L2](./diagrams/c4-containers.md).

## 2. Vista de componentes (mapeada a ADR-GRP-002)

| Componente del motor | Ubicación en el monorepo | Responsabilidad | ADR |
|----------------------|--------------------------|-----------------|-----|
| Proceso del motor | `apps/cli`: subcomando `raptor daemon` del binario `raptor` (PQ-5; sin app `raptord`) | Ciclo de vida, instancia única, parada ordenada; autoarranque solo con `raptor daemon enable` (PQ-1) | ADR-GRP-005 |
| Registro de repos y orquestación | `crates/core` | Repos observados y estados del motor ("Esperando Git", "Sin repos", "Observando") | ADR-GRP-005, ADR-GRP-009 |
| Observador de cambios | `crates/core` | Watcher `notify`, debounce fijo de 75 ms, sondeo de respaldo, modo degradado y reconciliación | ADR-GRP-010 |
| Cálculo de estado por worktree | `crates/core` sobre `crates/git` | Rama, cambios, estados especiales, ahead/behind; caché de stat en memoria | ADR-GRP-009, ADR-GRP-010 |
| Detección de agentes (adaptador Claude Code) | `crates/core` (módulo de adaptadores por agente) | Sesiones por S1; atribución con S2b, S3, S4 o el registro explícito si es la única sesión presente; transcripts limitados a metadatos (PQ-2) | ADR-GRP-012 |
| Modelo de eventos, sesiones y atribución | `crates/core` | Eventos inmutables, registros de atribución append-only (incluido el retiro de registro), huecos | ADR-GRP-013 |
| Almacén del perfil | `crates/core` (módulo de almacenamiento) | Carpetas estándar por SO (PQ-4, PQ-7), un SQLite por repo, único escritor: el daemon | ADR-GRP-006 |
| Capa de lectura de Git | `crates/git` | Único punto que toca repos: gitoxide en solo lectura, allowlist del Git CLI, resolución de Git ≥ 2.38 | ADR-GRP-009 |
| Configuración en tres niveles | `crates/policy` (compartido con Guardrails) | Documento JSON completo: carga, niveles admitidos, precedencia, diagnósticos; el motor consume `engine` | ADR-GRP-007, ADR-GRP-008 |
| Contrato y transporte de clientes | `crates/api` | JSON-RPC 2.0, handshake, stream con tiempos por etapa, socket Unix o named pipe, biblioteca cliente | ADR-GRP-005, ADR-GRP-011 |
| Clientes | `apps/cli` (CLI/TUI) y `apps/mcp` (`raptor-mcp`) | Consumen el contrato; nunca abren el perfil. La presentación es de F-001-02 y el MCP de F-001-05 | ADR-GRP-005 |
| — | `crates/theme` | No lo usa el motor | — |

⚠️ **ASSUMPTION**: el almacén y los adaptadores viven como módulos de `crates/core` para no crear crates fuera de ADR-GRP-002. Separarlos después exigiría enmendar ese ADR.

**Perfil** (ADR-GRP-006): las carpetas exclusivas de GitRaptor de datos (almacén), configuración (`settings.json` y `repos/<id-repo>/settings.local.json`, solo lectura para el motor, ADR-GRP-008), estado (bloqueo y logs) y ejecución (socket).

## 3. Skeleton Index

| ID | Tipo | Título | Path | Status |
|----|------|--------|------|--------|
| [ADR-GRP-005](./decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md) | ADR | Forma del motor: proceso por usuario y canal local | architecture/decisions/ADR-GRP-005-forma-motor-proceso-segundo-plano.md | expanded |
| [ADR-GRP-006](./decisions/ADR-GRP-006-perfil-ubicacion-almacenamiento.md) | ADR | Perfil: ubicación, clave de repo y almacenamiento | architecture/decisions/ADR-GRP-006-perfil-ubicacion-almacenamiento.md | expanded |
| [ADR-GRP-007](./decisions/ADR-GRP-007-configuracion-tres-niveles-formato.md) | ADR | Configuración en tres niveles: formato y precedencia | architecture/decisions/ADR-GRP-007-configuracion-tres-niveles-formato.md | expanded |
| [ADR-GRP-008](./decisions/ADR-GRP-008-configuracion-local-no-versionada.md) | ADR | Configuración local personal sin versionar | architecture/decisions/ADR-GRP-008-configuracion-local-no-versionada.md | expanded |
| [ADR-GRP-009](./decisions/ADR-GRP-009-frontera-solo-lectura-git.md) | ADR | Frontera de solo lectura e invocación de Git | architecture/decisions/ADR-GRP-009-frontera-solo-lectura-git.md | expanded |
| [ADR-GRP-010](./decisions/ADR-GRP-010-observacion-cambios-worktrees.md) | ADR | Observación de cambios en worktrees | architecture/decisions/ADR-GRP-010-observacion-cambios-worktrees.md | expanded |
| [ADR-GRP-011](./decisions/ADR-GRP-011-presupuesto-frescura.md) | ADR | Reparto del presupuesto de frescura | architecture/decisions/ADR-GRP-011-presupuesto-frescura.md | expanded |
| [ADR-GRP-012](./decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md) | ADR | Detección de sesiones de Claude Code | architecture/decisions/ADR-GRP-012-deteccion-sesiones-claude-code.md | expanded |
| [ADR-GRP-013](./decisions/ADR-GRP-013-modelo-eventos-atribucion.md) | ADR | Modelo de eventos, sesiones y atribución | architecture/decisions/ADR-GRP-013-modelo-eventos-atribucion.md | expanded |
| [TS-GRP-001](../requirements/features/motor-local/technical-stories/TS-GRP-001-almacen-perfil.md) | TS | Almacén de datos del motor en el perfil | requirements/features/motor-local/technical-stories/TS-GRP-001-almacen-perfil.md | expanded |
| [TS-GRP-002](../requirements/features/motor-local/technical-stories/TS-GRP-002-lectura-git.md) | TS | Capa de lectura de Git sin escrituras | requirements/features/motor-local/technical-stories/TS-GRP-002-lectura-git.md | expanded |
| [TS-GRP-003](../requirements/features/motor-local/technical-stories/TS-GRP-003-proceso-motor.md) | TS | Proceso del motor en segundo plano | requirements/features/motor-local/technical-stories/TS-GRP-003-proceso-motor.md | expanded |
| [TS-GRP-004](../requirements/features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md) | TS | Canal local de clientes y contrato | requirements/features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md | expanded |
| [INF-GRP-001](../requirements/features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md) | INF | Arnés "repo intacto" en los tres SO | requirements/features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md | expanded |
| [INF-GRP-002](../requirements/features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md) | INF | Banco de frescura y escala | requirements/features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md | expanded |
| [SPIKE-GRP-001](../requirements/features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md) | SPIKE | Precisión de detección de Claude Code | requirements/features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md | expanded |
| [SPIKE-GRP-002](../requirements/features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md) | SPIKE | Viabilidad del observador a escala | requirements/features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md | expanded |
| NFR-MOTOR | NFR | Requisitos no funcionales del motor y Security NFRs (SEC-01..14) | architecture/non-functional.md | expanded |
| [C4-GRP-L1](./diagrams/c4-context.md) | C4-L1 | Contexto del motor local | architecture/diagrams/c4-context.md | expanded |
| [C4-GRP-L2](./diagrams/c4-containers.md) | C4-L2 | Contenedores del motor local | architecture/diagrams/c4-containers.md | expanded |
| [SEQ-GRP-CAMBIO](./diagrams/seq-cambio-worktree.md) | Secuencia | Cambio en worktree → Cockpit (US-GRP-002) | architecture/diagrams/seq-cambio-worktree.md | expanded |
| [SEQ-GRP-DETECCION](./diagrams/seq-deteccion-sesion.md) | Secuencia | Detección de sesión de Claude Code (US-GRP-007, 008) | architecture/diagrams/seq-deteccion-sesion.md | expanded |
| SEQ-GRP-HUECO | Secuencia | Reconciliación tras un hueco (opcional; US-GRP-004, 005) | architecture/diagrams/seq-reconciliacion-hueco.md | draft |
| API-GRP-IPC | API Spec | Contrato del canal local (OpenRPC, no OpenAPI) | architecture/design/api-contract-ipc.md | draft |
| DATA-GRP-PERFIL | Data Model | Modelo de datos del perfil | architecture/design/data-model-perfil.md | draft |
| THREAT-GRP-MOTOR | Threat Model | STRIDE de la IPC local, archivos de terceros y perfil (delegar en `security-expert`) | architecture/security/threat-model-motor.md | draft |

## 4. Decisiones clave

Ver [decisions/index.md](./decisions/index.md): decisión de cada ADR, decisión de producto adoptada (PQ-1 a PQ-9) y grafo de dependencias.

## 5. Flujos críticos

- [Cambio en un worktree hasta el Cockpit](./diagrams/seq-cambio-worktree.md): etapas y presupuestos de ADR-GRP-011.
- [Detección de una sesión de Claude Code](./diagrams/seq-deteccion-sesion.md): señales y regla de combinación de ADR-GRP-012.
- Reconciliación tras un hueco: pendiente (SEQ-GRP-HUECO, opcional).

## 6. Atributos de calidad

Ver [non-functional.md](./non-functional.md). Los tres que más pesan en el diseño:

- **Cero escrituras en el repo**: NFR-01 y BR-CONS-001, con la frontera estricta de ADR-GRP-009 y el gate INF-GRP-001.
- **Continuidad sin huecos**: BR-CONS-005, con ADR-GRP-005 y ADR-GRP-010.
- **Frescura < 500 ms**: NFR-04, con el reparto de ADR-GRP-011 y el gate INF-GRP-002.

## 7. Enablers técnicos

Ver [technical-stories.md](../requirements/features/motor-local/technical-stories.md): 4 TS y 2 INF con status `Dev Spec Pending` y 2 SPIKE con status `Research Pending`. TS-GRP-003 solo crea el esqueleto de la máquina de estados de BR-WF-002; la entrada y la exposición de "Esperando Git" y "Sin repos" son de US-GRP-014 y US-GRP-015. INF-GRP-001 se divide en un núcleo (depende de TS-GRP-002) y suites incrementales que entran con su historia dueña.

### Trabajo técnico que vive en la US (no es TS)

Lo que tiene una sola historia dueña va en la Dev Spec de esa historia, según la regla del dueño:

| Trabajo técnico | Historia dueña | ADR |
|-----------------|----------------|-----|
| Watcher, recomputo incremental e histograma de frescura de dogfooding | US-GRP-002 | ADR-GRP-010, ADR-GRP-011 |
| Detección de estados especiales y worktree no disponible | US-GRP-003 | ADR-GRP-010 |
| `raptor daemon enable` y `disable`: registro del autoarranque por SO (excepción PQ-1) | US-GRP-004 | ADR-GRP-005 |
| Reconciliación tras un hueco | US-GRP-005 | ADR-GRP-010, ADR-GRP-013 |
| Adaptador de detección de Claude Code y modelo de sesión | US-GRP-007 | ADR-GRP-012, ADR-GRP-013 |
| Contrato de "quién hizo un evento" | US-GRP-009 | ADR-GRP-013 |
| Ahead/behind frente a la rama base, sin tocar el remoto | US-GRP-012 | ADR-GRP-009 |
| Lector de configuración en tres niveles y ruta expuesta de `settings.local.json` (coordinado con F-001-04) | US-GRP-013 | ADR-GRP-007, ADR-GRP-008 |
| Estado "Esperando Git": entrada, salida, recomprobación y su exposición (la resolución por candidatos es de TS-GRP-002 y el esqueleto de estados de TS-GRP-003) | US-GRP-014 | ADR-GRP-009, ADR-GRP-005 |
| Estado "Sin repos": entrada, salida y su exposición (esqueleto de estados de TS-GRP-003) | US-GRP-015 | ADR-GRP-005, ADR-GRP-006 |
| Suites incrementales del arnés "repo intacto" (INF-GRP-001): observación, autoarranque y `~/.claude` (las de proceso y canal van en TS-GRP-003 y TS-GRP-004) | US-GRP-002, US-GRP-004, US-GRP-007 | ADR-GRP-009 |

## 8. Índice de Dev Specs

Pendiente. Se generan con `/aadd-devspec <id>`: una por TS e INF y una por cada US de la tabla anterior. Los SPIKE llevan un Research Brief en `architecture/research/`, no una Dev Spec.

## 9. Preguntas abiertas y riesgos

- **Supuestos por confirmar**:
  - "< 500 ms" de NFR-04 como p95 en las máquinas de referencia (ADR-GRP-011; lo mide SPIKE-GRP-002).
  - La shell de Claude Code no tiene TTY interactiva (PQ-6 en ADR-GRP-005; lo comprueba SPIKE-GRP-001). Tras la revisión de seguridad deja de ser crítico: los comandos reservados se autorizan en el daemon (I4).
  - P16 y P17, abiertas para el PO, con supuesto "sí" en ADR-GRP-013.
- **Ruta crítica**: TS-GRP-001 y TS-GRP-002 → TS-GRP-003 → TS-GRP-004 → US-GRP-001 → US-GRP-002. El núcleo de INF-GRP-001 (tras TS-GRP-002) bloquea el merge de todas las historias; cada suite incremental bloquea solo el de su historia dueña. El Scrum Master debe recalcular las olas.
- **Riesgo de atribución**: si S2b se desactiva por un cambio de formato, los cambios sin commitear en un worktree con editor abierto quedan casi siempre "sin atribuir". La meta del 90% puede cumplirse para sesiones y no para cambios (R1, R2, R7). Vía de salida: S5 opt-in y registro explícito (ADR-GRP-012).
- **Riesgo de Windows**: los handles del watcher podrían impedir borrar o mover worktrees. Lo verifica SPIKE-GRP-002 (R4).

## 10. Pendientes fuera de esta rama

Esta rama (`docs/arch-motor-local`) solo toca arquitectura. Lo siguiente queda para otros dueños:

1. **PO — requerimiento de motor-local** (no se edita desde la arquitectura):
   - **PQ-1 (autoarranque)**: actualizar Q17, la verificación 2 de BR-CONS-001 ("fuera del repo, lo único que cambia son los datos del motor en el perfil") y el NFR "fuera del repo solo cambia el perfil" con la excepción acotada del autoarranque.
   - **PQ-3 (configuración local)**: actualizar la tabla de datos de BR-CONS-001 (fila de configuración local personal: "perfil, indexada por repo"), su punto 6 y BR-CONS-007 (nivel 3).
   - **Perfil**: confirmar que "perfil" abarca las carpetas exclusivas de GitRaptor de datos, configuración, estado y ejecución (ADR-GRP-006).
   - **P16 y P17**: responder si una confirmación pasa el origen a "registrado" y si retirar una corrección devuelve sus eventos (ADR-GRP-013).
   - **Retirar una corrección con la sesión ya terminada**: BR-CONS-002 no lo cubre; ADR-GRP-013 supone que se resuelve igual que con la sesión presente.
   - **BR-WF-001 (actividad)**: dejar explícito que las ediciones del humano también mantienen activa la sesión y que el mtime de transcripts no cuenta como actividad (ADR-GRP-012).
   - **`--resume` / `--continue`**: dejar explícito que es una sesión nueva (ADR-GRP-012, Q41).
   - **Retiro de registro (ASSUMPTION)**: confirmar que el desarrollador puede retirar cualquier registro y un agente solo el suyo (worktree = cwd del llamante, igual que al registrarse), y añadir "retirar su registro" a la fila del agente en la tabla de BR-AUTH-001 (ADR-GRP-005 § 6, ADR-GRP-013 § 2).
   - **Registro explícito como evidencia**: dejar explícito en BR-EDGE-004 que el registro atribuye los eventos del worktree mientras sea la única sesión presente, y que en un worktree compartido rige la evidencia por evento (ADR-GRP-012, ADR-GRP-013 § 3).
   - **Cerrar P8, P9 y P10 en el context**: los resuelven ADR-GRP-007, ADR-GRP-006 y ADR-GRP-008. Con P8 cerrada, **desbloquear US-GRP-013** (umbral por repo); **actualizar el bloqueo de US-GRP-016**, que pasa a depender solo de Guardrails (F-001-04).
   - **Q26 / BR-CONS-005**: el daemon **aborta** si los permisos del perfil o del socket están alterados (ADR-GRP-005 § 5, ADR-GRP-006 § 1, SEC-01, SEC-06). Choca con "0 huecos mientras la máquina está encendida"; confirmar con el PO que se acepta ese hueco, que queda señalado.
   - **NFR-07 del BRD**: su literal ("respeta config/hooks") choca con la neutralización de filtros, `textconv` y fsmonitor al leer (ADR-GRP-009). Reformularlo como "respeta la configuración de Git sin ejecutar programas configurados por el usuario".
2. **Cockpit (F-001-02) — predicción de conflictos**: `git merge-tree --write-tree`, la razón de Git 2.38 en NFR-07, escribe objetos en `.git/objects` y ADR-GRP-009 lo prohíbe al motor. Necesita un ADR propio del Cockpit (por ejemplo, un almacén de objetos alternativo dentro del perfil).
3. **Guardrails (F-001-04)**:
   - Un `.gitraptor/settings.json` de equipo inválido se ignora entero (PQ-8), lo que deja sin efecto sus prohibiciones (fail-open). Guardrails debe decidir su reacción.
   - Un repo bare no tiene configuración de equipo (PQ-9); Guardrails debe decidir si necesita la misma regla.
   - ADR-GRP-007 no pasa a `accepted` sin el context de Guardrails (coautoría de `permissions` y `policies`).
4. **Seguridad de la salida hacia terminales y agentes (M8, SEC-12)**: ADR-GRP-005 § 5 fija el contrato de salida de `crates/api` (texto no confiable marcado, respuestas MCP acotadas). Falta llevarlo a **ADR-GRP-004** (limpieza de caracteres de control y escapes ANSI/OSC en la CLI/TUI) y a la **spec futura del MCP (F-001-05)** (allowlist de campos, longitudes máximas, sin mensajes de commit ni contenido, limitado al repo del llamante). La enmienda de ADR-GRP-004 hecha en esta rama se limita a sustituir la referencia a `policy.yaml` por ADR-GRP-007; **M8/SEC-12 sigue pendiente** en ADR-GRP-004.
5. **Código y README** (fuera del alcance de una rama `docs/`): todavía citan `policy.yaml` en `README.md` (tabla de crates), `crates/policy/src/lib.rs` (comentario del crate) y `crates/policy/Cargo.toml` (`description`). Deben pasar a citar la configuración de ADR-GRP-007.
