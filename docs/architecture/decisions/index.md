---
mode: draft
status: accepted
generated: 2026-10-03
updated: 2026-10-06
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 34
expanded: 34
approved: 32
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-007, ADR-GRP-008, ADR-GRP-009, ADR-GRP-010, ADR-GRP-011, ADR-GRP-012, ADR-GRP-013, ADR-GRP-014, ADR-GRP-015, ADR-GRP-016, ADR-TMC-001, ADR-TMC-002, ADR-TMC-003, ADR-TMC-004, ADR-TMC-005, ADR-TMC-006, ADR-TMC-007, ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-GRD-007, ADR-GRD-008, ADR-CKP-001, ADR-CKP-002, ADR-CKP-003]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016]
---

# Índice de ADRs — GitRaptor (GRP)

## Decisiones de producto PQ-1..PQ-9 (Rene Bonilla, 2026-10-03)

Fuente única de los identificadores PQ que citan los ADR 005 a 013.

| ID | Decisión | ADR |
|----|----------|-----|
| PQ-1 | El autoarranque es una **excepción acotada a Q17**: solo lo registra el instalador o `raptor daemon enable` | ADR-GRP-005 |
| PQ-2 | Leer `~/.claude/projects` **limitado a metadatos** (herramienta, ruta de archivo, marca de tiempo, id de sesión y cwd) | ADR-GRP-012 |
| PQ-3 | La configuración local personal vive **en el perfil**, indexada por repo | ADR-GRP-008 |
| PQ-4 | **Nombres**: `.gitraptor/settings.json`, `settings.local.json`, sección `engine` y perfil en la carpeta estándar de cada SO (no `~/.gitraptor`) | ADR-GRP-006, ADR-GRP-007 |
| PQ-5 | El daemon es el **subcomando `raptor daemon`** (sin app `raptord`) | ADR-GRP-005 |
| PQ-6 | **Comandos reservados**: rechazo si el llamante desciende de un agente, más confirmación | ADR-GRP-005 |
| PQ-7 | Windows siempre en **`%LOCALAPPDATA%`**, también la configuración | ADR-GRP-006 |
| PQ-8 | Un nivel de configuración con **JSON inválido se ignora entero**, con diagnóstico | ADR-GRP-007 |
| PQ-9 | ~~Manda la **configuración de equipo del worktree principal**~~ — **sustituida por la decisión 1 de Guardrails** (Rene Bonilla, 2026-10-04): el equipo se lee de lo commiteado; la rama base y las relajaciones, solo de la copia de la rama principal; la rama base efectiva es la confirmada por el humano | ADR-GRP-007, ADR-GRD-004 |

El formato "JSON estricto con `$schema`" no es una PQ: es la propuesta base del BRD v0.4, que ADR-GRP-007 adopta.

> Los ADR 001 a 004 están aceptados. Los ADR 005 a 013 de la feature `motor-local` están **aceptados** (`status: accepted`, Rene Bonilla, 2026-10-04), igual que los ADR-GRD-001 a 007 de Guardrails. Rene Bonilla ya había aceptado el 2026-10-03 todas las recomendaciones y las preguntas de producto PQ-1 a PQ-9 (PQ-9 sustituida el 2026-10-04). ADR-GRP-007 cierra P8. Este índice solo resume la decisión; el detalle vive en cada archivo.
>
> **Enmiendas de Guardrails (2026-10-04)**: ADR-GRP-005, 006, 007, 009, 010 y 013 e INF-GRP-001 incorporan las enmiendas que pedía la arquitectura de Guardrails (ADR-GRD-001 a 007), cada una con su sección "Enmienda (2026-10-04, Guardrails)". Todos quedaron aceptados el 2026-10-04 con sus enmiendas. Lista y estado en [non-functional-guardrails.md](../non-functional-guardrails.md#enmiendas-pendientes-en-otros-frentes-j10).
>
> **Cockpit (2026-10-04)**: los ADR-CKP-001 a 003 de la feature `cockpit` están **aceptados** (`status: accepted`, 2026-10-04). Decisión del orquestador (2026-10-04), validada por Arquitecto, PO y security-expert. ADR-CKP-001 se acepta con el mecanismo condicionado a SPIKE-CKP-001, que lo enmendará con sus resultados (mismo patrón que ADR-GRD-001 y 002 con SPIKE-GRD-001). Sus enmiendas a otros ADR llevan la marca "Enmienda (2026-10-04, Cockpit)" en el ADR de destino, y su estado está en la tabla de enmiendas de cada ADR-CKP.
>
> **Restricciones activas** (no hay `architecture-constitution.md` en la cascada): ADR-GRP-001 (Rust; gitoxide para leer y Git CLI para escribir; ratatui, clap, rmcp) y ADR-GRP-002 (Nx package-based; `crates/{core,policy,git,api,theme}`, `apps/{cli,mcp}`). ⚠️ **ASSUMPTION**: se tratan como constitución mientras no exista una formal (`/aadd-architect --init-constitution`).

## Outline

| ID | Título | Decisión (1 línea) | Status |
|----|--------|--------------------|--------|
| [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md) | Stack tecnológico | Motor, CLI/TUI y MCP en Rust; Tauri + React solo en la Fase 3 | accepted |
| [ADR-GRP-002](./ADR-GRP-002-monorepo-nx.md) | Monorepo Nx package-based | pnpm + Cargo workspaces con `@monodon/rust`, crates y apps por fase | accepted |
| [ADR-GRP-003](./ADR-GRP-003-design-system.md) | Design system | Tokens, UI kit y patrones; en el MVP solo tokens y tema de TUI | accepted |
| [ADR-GRP-004](./ADR-GRP-004-estado-frontend-ux.md) | Estado del frontend y UX | Estado React de la Fase 3; en el MVP solo los patrones de UX | accepted |
| [ADR-GRP-005](./ADR-GRP-005-forma-motor-proceso-segundo-plano.md) | Forma del motor: proceso por usuario y canal local | Subcomando `raptor daemon` (PQ-5) con autoarranque registrado solo por instalador o comando del desarrollador (PQ-1), IPC local JSON-RPC y comandos reservados (incluida la parada del daemon) autorizados por el daemon según la ascendencia del llamante (PQ-6); la confirmación en terminal es solo UX | accepted |
| [ADR-GRP-006](./ADR-GRP-006-perfil-ubicacion-almacenamiento.md) | Perfil: ubicación por SO, clave de repo y almacenamiento | Carpetas estándar por SO (PQ-4), Windows siempre en `%LOCALAPPDATA%` (PQ-7), UUID por directorio Git común y un SQLite por repo | accepted |
| [ADR-GRP-007](./ADR-GRP-007-configuracion-tres-niveles-formato.md) | Configuración en tres niveles: formato y precedencia | JSON estricto con `$schema` (propuesta base del BRD v0.4), nombres de archivos y sección `engine` (PQ-4), niveles admitidos por clave, JSON o schema inválido ignora el nivel entero (PQ-8); equipo leído de lo commiteado con suelo en la rama principal y rama base confirmada (decisión 1 de Guardrails, sustituye a PQ-9); `permissions`/`policies` y estado por fuente (enmienda 2026-10-04) | accepted |
| [ADR-GRP-008](./ADR-GRP-008-configuracion-local-no-versionada.md) | Configuración local personal sin versionar | `settings.local.json` en el perfil, indexado por repo (PQ-3); P10 desaparece | accepted |
| [ADR-GRP-009](./ADR-GRP-009-frontera-solo-lectura-git.md) | Frontera de solo lectura e invocación del Git del sistema | Frontera estricta: cero escrituras (ni locks transitorios), cero programas del usuario, gitoxide + allowlist del CLI; Git ≥ 2.38 sin depender del PATH | accepted |
| [ADR-GRP-010](./ADR-GRP-010-observacion-cambios-worktrees.md) | Observación de cambios en worktrees | Watcher nativo (`notify`), debounce fijo de 75 ms, recomputo incremental, sondeo de respaldo, modo degradado y reconciliación; enmienda 2026-10-04 de SPIKE-GRP-002 (validado en macOS; Linux y Windows pendientes) | accepted |
| [ADR-GRP-011](./ADR-GRP-011-presupuesto-frescura.md) | Reparto del presupuesto de frescura | Motor ≤ 300 ms, Cockpit ≤ 100 ms y 100 ms de margen, p95 medido con reloj monótono por etapa; enmienda 2026-10-04 de SPIKE-GRP-002 (p95 confirmado en macOS; Linux y Windows pendientes) | accepted |
| [ADR-GRP-012](./ADR-GRP-012-deteccion-sesiones-claude-code.md) | Detección de sesiones de Claude Code | S1 (proceso y cwd) crea la sesión; atribuyen S2b, S3, S4 o el registro explícito de un "otro agente" si es la única sesión presente (confirmar una sesión detectada no activa esa evidencia); la co-ubicación de una sesión detectada nunca basta; transcripts limitados a metadatos (PQ-2); enmienda 2026-10-06 de autoría de commits: separa quién ejecutó de la autoría declarada (autor, committer, `Co-Authored-By`), la pista `inferred` se valida contra el trailer y no existe con `human-author` (pendiente de BR-26 / US-GRD-018 / US-GRD-019) | accepted |
| [ADR-GRP-013](./ADR-GRP-013-modelo-eventos-atribucion.md) | Modelo persistido de eventos, sesiones y atribución | Los eventos apuntan a una sesión; registros de atribución append-only (incluido el retiro de registro, que termina la sesión); huecos como intervalos; sin variante "humano"; enmienda 2026-10-06: autoría declarada del commit como dato del evento, aparte del actor | accepted |
| [ADR-GRP-014](./ADR-GRP-014-pipeline-release-distribucion.md) | Pipeline de release y canales de distribución | Workflow propio (no cargo-dist) disparado por tag, 6 targets nativos con Linux musl estático, checksums, SBOM y borrador de release que publica un humano; firma de macOS y Windows y attestations preparadas tras secretos y variables; canales Homebrew (tap propio), winget, npm (paquetes por plataforma) y scripts verificados, publicados solo con `RELEASE_PUBLISH_CHANNELS`; nombre `gitraptor` en los canales y licencia FSL-1.1-ALv2 comprobada en CI | accepted |
| [ADR-GRP-015](./ADR-GRP-015-consumo-recursos.md) | Consumo de recursos: clases de trabajo, ahorro de energía y presupuesto de huella | Cuatro clases de trabajo (`user-initiated`, `default`, `utility`, `background`) con prioridad del SO por hilo y heredada por los `git` hijos; el snapshot previo y Guardrails nunca bajan; `engine.powerSaving` (reconciliación cada 15 min y predictor en pausa con batería); HUELLA dividida en RES-01 a RES-05 con gate en INF-GRP-002; `engine.resources` para `raptor status --resources` | proposed |
| [ADR-GRP-016](./ADR-GRP-016-extension-registro-capacidades.md) | Extensión por registro y negociación de capacidades | Protocolo congelado en 9 para cambios aditivos: métodos descubiertos en `hello.methods` y cambios de forma como capacidades con nombre (`connection.accept`); registro de métodos, capacidades y bloques de 20 códigos de error en un archivo por módulo del contrato; i18n por feature; subcomandos de la CLI y módulos del daemon registrables; test de arquitectura contra declaraciones en archivos centrales | accepted |
| [ADR-TMC-001](./ADR-TMC-001-almacen-snapshots-perfil.md) | Almacén de snapshots en el perfil | Repo Git bare privado por repo en `tm/<id-repo>/` con objetos propios, contenido en bruto sin filtros y exclusiones declaradas; nunca en el repo del usuario | accepted |
| [ADR-TMC-002](./ADR-TMC-002-escritor-time-machine.md) | Escritor de la Time Machine | Escrituras internas en el módulo `timemachine` del daemon con capa de escritura propia en `crates/git`, sin hooks, filtros, firma ni red; las operaciones de usuario las ejecuta el ejecutor del daemon | accepted |
| [ADR-TMC-003](./ADR-TMC-003-oplog-diario-recuperacion.md) | Oplog, diario y recuperación | Oplog SQLite propio por repo, solo por anexión y encadenado por hash; solicitante congelado; recuperación sin escrituras propias salvo liberar su `index.lock` | accepted |
| [ADR-TMC-004](./ADR-TMC-004-cobertura-dos-niveles.md) | Cobertura en dos niveles | Operación protegida como único camino de escritura; captura por observación sobre eventos publicados del motor; previo vía hook para Guardrails | accepted |
| [ADR-TMC-005](./ADR-TMC-005-solicitante-permisos-solape.md) | Solicitante, permisos y solape | Solicitante por ascendencia en el daemon (agente X o sin atribuir); reto ligado al plan; Guardrails solo deniega; solape por archivo y ref | accepted |
| [ADR-TMC-006](./ADR-TMC-006-presupuesto-rendimiento-snapshot.md) | Presupuesto del snapshot (NFR-04) | p95 < 200 ms del snapshot previo con almacén sembrado; repo mediano fijado por SPIKE-TMC-001; gate en el banco de INF-GRP-002 | accepted |
| [ADR-TMC-007](./ADR-TMC-007-retencion-purga-segura.md) | Retención y purga segura | `timeMachine.retentionDays` (perfil y local, 30); protección del previo a la última destructiva; purga en dos fases con aviso; solo refs del almacén; Enmienda 2026-10-05: tope total `timeMachine.maxDiskSizeGiB` (10) con purga de lo más antiguo no protegido, aviso y gracia | accepted |
| [ADR-GRD-001](./ADR-GRD-001-capa-hooks-instalacion.md) | Capa de hooks: instalación, encadenado y worktrees | Dispatchers propios en el directorio Git común activados con `core.hooksPath` absoluto; entorno por allowlist y encadenado del hook previo sin moverlo ni editarlo; instalación transaccional con un único punto de commit, ejecutada por el daemon tras un comando reservado; referencia de integridad en el perfil; enmienda 2026-10-04 de SPIKE-GRD-001 (conjunto mínimo de dispatchers, dispatcher nativo para `reference-transaction` desde Git 2.54, criterio semántico de desinstalación; validado en macOS, Linux y Windows pendientes) | accepted |
| [ADR-GRD-002](./ADR-GRD-002-operaciones-interceptables.md) | Operaciones interceptables y límites de los hooks | Cada operación la gobierna el hook que corre antes de sus efectos, con `reference-transaction` en `prepared` como segunda línea que `--no-verify` no salta; toda ref fuera de la lista de excepciones es gobernada; lecturas sin objetos de reemplazo; lista publicada de lo no impedible versionada en el binario; enmienda 2026-10-04 de SPIKE-GRD-001 (matriz confirmada en macOS, *prune* de `pack-refs`, solo `prepared`, presupuesto por comando, renombrado de la base no impedible en repos reftable; Linux y Windows pendientes) | accepted |
| [ADR-GRD-003](./ADR-GRD-003-motor-decision-contrato.md) | Motor de decisión, mínimo seguro y contrato | Función pura y determinista en `crates/policy`, alojada por el daemon y expuesta por el canal (`crates/api`); el cliente del hook solo habla con un daemon autenticado por una ruta fijada al instalar; actor e identidad del `git` antecesor más cercano; sin daemon, modo degradado más estricto que no lee el perfil | accepted |
| [ADR-GRD-004](./ADR-GRD-004-configuracion-efectiva.md) | Configuración efectiva para Guardrails | Cargador único en `crates/policy` que lee el equipo de objetos commiteados sin objetos de reemplazo; la copia de la rama principal es el suelo, única fuente de relajaciones y de la rama base; el `HEAD` del worktree solo endurece; un cambio del suelo que baje la protección espera la confirmación del humano | accepted |
| [ADR-GRD-005](./ADR-GRD-005-estado-proteccion.md) | Estado de protección y detección de pérdida | Estado derivado, no guardado, de dos capas: hooks verificados por worktree contra la referencia de integridad del diario y repo en la allowlist del MCP (F-001-05); pérdida detectada por el observador del motor o por comprobación periódica; cada transición al registro y aviso con rebote solo si la pérdida no la hizo Guardrails | accepted |
| [ADR-GRD-006](./ADR-GRD-006-registro-decisiones.md) | Registro de decisiones (90 días) | Tabla propia de Guardrails en el almacén por repo del perfil, separada de los eventos inmutables; la escribe el daemon con agregación y límite de inserciones, se purga a los 90 días y se consulta por el canal; instalaciones, desinstalaciones, adopciones e intentos de comandos reservados también van a la auditoría de ADR-GRP-013 | accepted |
| [ADR-GRD-007](./ADR-GRD-007-acciones-reservadas-excepcion.md) | Acciones reservadas al humano y excepción consciente | Amplían los comandos reservados de ADR-GRP-005 § 6 con su mismo mecanismo en el daemon; las que relajan añaden anuncio, ventana cancelable y auditoría en el MVP, y factor fuera de banda antes de US-GRD-013 y US-GRD-015; la excepción es un token de un solo uso ligado al `git` hijo directo del `raptor` que lo pidió y a la transición exacta | accepted |
| [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) | Factor de autenticación del sistema operativo fuera del canal del agente | El daemon invoca el diálogo del SO (LocalAuthentication, Windows Hello en Windows 11, polkit con agente gráfico), ligado a un reto de un solo uso con el resumen del plan; la prueba nunca cruza el canal; fail-closed si no está disponible; obligatorio para relajar con el comando (US-GRD-013) y aprobar en la cola (US-GRD-015); 9 OQ resueltas por el orquestador con Arquitecto/PO; trinquete de niveles personales (Q-GRD-32); SPIKE-GRD-002 gatea las Dev Specs y el binding | accepted |
| [ADR-CKP-001](./ADR-CKP-001-prediccion-conflictos-merge-en-seco.md) | Predicción de conflictos con merge en seco sin escribir en el repo | Predictor en el daemon con dos niveles (solape ⚠ y conflicto previsto ⚡) por par, recálculo incremental con cola coalescente y estados con hora de cálculo; merge en memoria con gitoxide como opción preferida (sin drivers, filtros, atributos, reemplazos ni descargas, con cotas de tiempo y memoria), `merge-tree` sobre un almacén del perfil como respaldo y contra el repo rechazado; mecanismo condicionado a SPIKE-CKP-001 | accepted |
| [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) | Catálogo de operaciones de usuario y ejecutor del daemon | Catálogo cerrado y versionado de ocho operaciones con marcas Cockpit y MCP; preparar (plan, `planId` ligado a la conexión, huella) y ejecutar bajo el cerrojo del repo dentro de la operación protegida; capa fijada por el daemon según el solicitante y decisión de Guardrails registrada una vez; `git` hijo directo tras una barrera de arranque, sin TTY ni shell; el canal rechaza lo reservado a los descendientes del daemon (`daemon-descendant`) y el ejecutor los atribuye al solicitante del plan; rebase `stop` (Cockpit) o `atomic` (MCP) | accepted |
| [ADR-CKP-003](./ADR-CKP-003-arquitectura-tui.md) | Arquitectura de la TUI y de la CLI de solo lectura | TEA (`update` y `view` puros) con un bucle de un hilo y render coalescido; cliente del canal con instantánea N y suscripción N+1, resync y reconexión, y comprobación del par antes del handshake; saneado SEC-12 en un único punto por tipo; 100 ms p95 instrumentado con gate; tema agnóstico de `ratatui`; módulos de `apps/cli` sin crates nuevos | accepted |
| [ADR-MCP-001](./ADR-MCP-001-servidor-mcp-cliente-daemon.md) | Servidor MCP como cliente del daemon | `raptor-mcp` por stdio con `rmcp`, solo `tools`; ámbito por el cwd y solicitante por ascendencia, resueltos por el daemon en cada llamada; perfil `mcp` por solicitante agente sea cual sea el cliente; allowlist como marca reservada del repo observado; diez herramientas sobre el canal y el catálogo de ADR-CKP-002, avisos reconocidos en una segunda llamada; respuestas con allowlist de campos y 24 KiB por parte; cifras de S-MCP-1; OWASP MCP Top 10 | accepted |

## Grafo de dependencias entre ADRs nuevos

| ADR | Depende de | Lo consumen |
|-----|-----------|-------------|
| ADR-GRP-005 | 006 (socket, bloqueo y logs), 009 (resolución de Git), 012 (procesos de agente, PQ-6), 013 (huecos y auditoría) | 006, 007, 008, 009, 010, 011, 012, 013 |
| ADR-GRP-006 | 005 (único escritor y canal) | 005, 007, 008, 009, 011, 012, 013 |
| ADR-GRP-007 | 005, 006 (carpeta de configuración del perfil; rama base y suelo confirmados); ADR-GRD-003 y ADR-GRD-004 (coautoría cerrada) | 008, 009 (`gitPath`), 010 (intervalos del watcher), 012 (umbral) |
| ADR-GRP-008 | 005, 006, 007 | — |
| ADR-GRP-009 | 005 (proceso y entorno heredado), 006, 007 (`gitPath`) | 005, 010, 011, 012 |
| ADR-GRP-010 | 005, 007, 009 | 011, 012, 013 (huecos) |
| ADR-GRP-011 | 005, 006, 009, 010, 013 | — (Cockpit F-001-02 lo consume) |
| ADR-GRP-012 | 005, 006, 007 (umbral), 009, 010 | 005 (ascendencia), 013 — lo valida SPIKE-GRP-001 |
| ADR-GRP-013 | 005 (único escritor), 006 (almacén), 010 (reconciliación), 012 (señales y evidencia) | 005 (huecos y auditoría), 011 |
| ADR-GRP-015 | 005 (autoarranque), 007 (`engine.powerSaving`), 010 (reconciliación), 011 (NFR-04); CKP-001 (predictor); TMC-004, 006, 007 | — (lo implementan TS-GRP-005, US-GRP-017 y INF-GRP-002) |
| ADR-GRP-016 | 002 (monorepo), 005 (§ 4 y § 5, protocolo y reemplazo); CKP-003 (§ 4 N7, § 10) | 005, CKP-003 (enmiendas del 2026-10-06) |
| ADR-TMC-001 | ADR-GRP-006 (perfil), 009 (lectura sin escrituras), 010 (filtros de ignorados y eventos) | TMC-002, 003, 004, 006, 007 |
| ADR-TMC-002 | ADR-GRP-001, 002, 005 (daemon y canal), 006 (único escritor), 009 (reglas de invocación); TMC-001 | TMC-003, 004, 005 |
| ADR-TMC-003 | ADR-GRP-006, 013 (atribución vigente); TMC-001, 002 | TMC-004, 005, 007 |
| ADR-TMC-004 | ADR-GRP-005, 010, 011 (presupuesto del motor), 012, 013; TMC-001, 002, 003 | TMC-006 |
| ADR-TMC-005 | ADR-GRP-005 § 6 (controles del daemon), 007 (políticas), 012 (procesos de agente), 013; TMC-002, 003 | — |
| ADR-TMC-006 | ADR-GRP-001, 011 (reloj y banco); TMC-001, 004 | — (lo valida SPIKE-TMC-001) |
| ADR-TMC-007 | ADR-GRP-007 (sección `timeMachine`), 008; TMC-001, 003 | — |
| ADR-GRD-001 | ADR-GRP-001, 002 (stack), 005, 006, 009, 013; ADR-TMC-002; GRD-002 (§ 4 entrada normalizada), 003 (§ 4 canal y modo degradado), 005 (detección de pérdida), 006 (registro), 007 (comando reservado y token) | GRD-002, 003, 005, 007 |
| ADR-GRD-002 | ADR-GRP-011 (criterio p95); ADR-TMC-004 § 3 (snapshot previo); GRD-001 (§ 2 constantes del dispatcher), 003 (§ 4 canal autenticado), 004 (forja de la configuración), 005 (pérdida y exposición de la lista) | GRD-001, 003, 005, 006, 007 |
| ADR-GRD-003 | ADR-GRP-005, 006, 007, 012, 013; ADR-TMC-002, 004, 005; GRD-001 (§ 2 constantes), 002 (§ 4 transiciones, § 5 presupuesto), 004 (configuración efectiva y rama base), 005 (estado y ventana degradada), 006 (spool y registro), 007 (excepción) | GRD-001, 002, 005, 006, 007; ADR-GRP-007 (coautoría) |
| ADR-GRD-004 | ADR-GRP-006 (§ 4 almacén por repo), 007 (PQ-8, PQ-9), 008, 009, 010; GRD-005 (diagnóstico `base-change-pending`), 007 (confirmación por comando reservado) | GRD-002, 003, 005, 007; ADR-GRP-007 (coautoría); GRD-008 |
| ADR-GRD-005 | ADR-GRP-005, 006, 010, 013; GRD-001 (diario, § 2 y § 8), 002 (lista publicada), 003 (§ 4 modo degradado), 004 (configuración ilegible y pendientes), 006 (registro), 007 (adopción y confirmación) | GRD-001, 002, 003, 004, 007 |
| ADR-GRD-006 | ADR-GRP-005, 006, 013 (auditoría permanente); GRD-002 (§ 4 rama normalizada), 003 (§ 3 efectos, § 4 spool, § 6 correlación), 007 (§ 3 excepción cancelada) | GRD-001, 003, 005, 007, 008 |
| ADR-GRD-007 | ADR-GRP-005 § 6, 009 § 4, 012, 013; GRD-001 (§ 2 encadenado), 002 (§ 4 refs y alias), 003 (§ 3 y § 6), 004 (§ 3 confirmaciones), 005 (§ 1 adopción), 006 (§ 1 registro) | GRD-001, 003, 004, 005, 006, 008 |
| ADR-GRD-008 | ADR-GRP-005 § 3 y § 6, 006, 012, 013 § 1; ADR-TMC-005 (SEC-TMC-03); GRD-004 (§ 2 y § 4 niveles y suelo), 006 (§ 1 registro), 007 (§ 1 a § 3, padre) | GRP-005 (§ 6), GRP-013 (§ 1), GRD-004 (§ 2, trinquete), GRD-007 (§ 1 y § 2) |
| ADR-CKP-001 | ADR-GRP-001, 002, 005, 006, 009, 010, 011, 013; ADR-GRD-006 (forma del registro y la purga); ADR-TMC-001 (alternates descartados) | CKP-003 (presentación del ⚡); Servidor MCP F-001-05 (`check_conflicts`) — lo valida SPIKE-CKP-001 |
| ADR-CKP-002 | ADR-GRP-001, 002, 005, 006, 007, 008, 009, 012, 013; ADR-TMC-001, 002, 003, 004, 005, 007; ADR-GRD-002, 003, 006, 007; CKP-003 (editor) | CKP-003; Servidor MCP F-001-05 (herramientas de escritura) |
| ADR-CKP-003 | ADR-GRP-001, 002, 003, 004, 005, 006, 009, 011, 013; ADR-TMC-004, 005; ADR-GRD-007; CKP-001, CKP-002 | CKP-002 (editor y salida de Git) |

Grafo derivado de la sección Referencias de cada ADR. ADR-GRP-005 es el proceso que aloja al resto, así que sus dependencias con 006, 009, 012 y 013 son mutuas: él usa sus rutas, su Git, su ascendencia y sus huecos, y ellos corren dentro del daemon. Las aristas de los ADR-TMC y ADR-GRD hacia los ADR-GRP (y de los ADR-GRD hacia los ADR-TMC) se listan solo en las filas TMC y GRD; las filas GRP y TMC no se modifican. Entre los ADR-GRD, la sección Referencias solo cita otros frentes, así que sus aristas internas salen de las menciones explícitas: "depende de" significa que el ADR cita al otro, y "lo consumen" es la relación inversa. La mayoría son mutuas porque la capa de hooks, el motor de decisión, el estado y el registro se apoyan unos en otros. La arista de ADR-GRP-007 hacia GRD-003 y 004 es la que ya figura en su fila.

### Feature time-machine

Los ADR-TMC-001 a 007 (aceptados el 2026-10-03) son de la feature `time-machine` (F-001-03). Su resumen, las decisiones TQ-1 a TQ-17 y el mapa de historias están en el [overview de la Time Machine](../time-machine/overview.md).

### Feature guardrails

Los ADR-GRD-001 a 007 (aceptados el 2026-10-04) son de la feature `guardrails` (F-001-04). Su NFR, los riesgos residuales y la tabla de enmiendas en otros frentes están en [non-functional-guardrails.md](../non-functional-guardrails.md). Las decisiones Q-GRD-1 a Q-GRD-27, que incluyen D5 a D12, y el contexto de la feature están en [context.md](../../requirements/features/guardrails/context.md#decisiones-tomadas).

### Feature cockpit

Los ADR-CKP-001 a 003 (aceptados el 2026-10-04) son de la feature `cockpit` (F-001-02). Las decisiones Q-CKP-1 a Q-CKP-30 y las dependencias DEP-CKP-1 a 14 están en el [contexto de la feature](../../requirements/features/cockpit/context.md#decisiones-tomadas). Los enablers (SPIKE-CKP-001, TS-CKP-001 a 004 e INF-CKP-001), su DAG y el destino de cada DEP-CKP están en el [índice de technical stories](../../requirements/features/cockpit/technical-stories.md).

- Componentes: [c4-ckp-components.md](../diagrams/c4-ckp-components.md).
- Secuencias: [predicción de conflictos](../diagrams/seq-ckp-prediccion.md) (ADR-CKP-001), [operación de usuario desde la TUI](../diagrams/seq-ckp-operacion-usuario.md) (ADR-CKP-002) y [arranque de la TUI y reconexión](../diagrams/seq-ckp-arranque-tui.md) (ADR-CKP-003).

---

## Resumen de cada ADR nuevo

### ADR-GRP-005 — Forma del motor: proceso por usuario y canal local

- **Pregunta**: ¿cómo observa el motor sin una superficie abierta (BR-CONS-005, Q1), cómo arranca y cómo se conectan la CLI/TUI y el MCP?
- **Decisión**: un daemon por usuario, subcomando `raptor daemon` del binario `raptor` (sin app `raptord`; ADR-GRP-002 no cambia). Instancia única por bloqueo en el perfil. Socket Unix 0600 (macOS, Linux) o named pipe con DACL del SID (Windows), sin TCP. JSON-RPC 2.0 en `crates/api` con handshake de versión y stream de eventos. Arranque bajo demanda como red de seguridad.
- **Decisiones de producto (Rene Bonilla, 2026-10-03)**:
  - **PQ-1**: el autoarranque (LaunchAgent, `systemd --user`, `Run` de HKCU) es una **excepción explícita y acotada a Q17**. Solo lo registra el instalador o `raptor daemon enable`, nunca el motor por su cuenta, y se revierte al desinstalar.
  - **PQ-5**: subcomando `raptor daemon`.
  - **PQ-6**: añadir o retirar repos, corregir y parar el daemon se rechazan si el llamante desciende de un agente detectado, y `raptor-mcp` no los expone. La autorización la hace **el daemon** (identificador no reutilizable, terminal de control y líder de sesión); la confirmación interactiva en terminal es solo UX. El supuesto de TTY de SPIKE-GRP-001 deja de ser crítico.
- **Impacta**: US-GRP-001, 002, 004, 005, 014, 015 y el canal de todas. BR-CONS-005, BR-WF-002, BR-AUTH-001.

### ADR-GRP-006 — Perfil: ubicación por SO, clave de repo y almacenamiento

- **Pregunta** (P9): ¿dónde vive el perfil, cómo se separan los repos y en qué formato?
- **Decisión**: crate `directories`; el perfil son las carpetas exclusivas de GitRaptor de datos, configuración, estado y ejecución. Clave de repo: UUID indexado por la ruta canónica del directorio Git común, con el commit raíz como pista. Un SQLite (`rusqlite`, WAL) por repo más un índice global; único escritor, el daemon. Archivo corrupto → se aparta y el repo se trata como perfil perdido. Variable de sobreescritura para tests.
- **Decisiones de producto**: **PQ-4** carpeta estándar por SO (no `~/.gitraptor`); **PQ-7** Windows siempre en `%LOCALAPPDATA%`, también la configuración.
- **Impacta**: US-GRP-001, 004, 005, 006, 009, 010, 011, 015. BR-CONS-001, BR-CONS-005, BR-AUTH-001, BR-EDGE-005, BR-EDGE-007. Q21, Q25, Q26, Q31.

### ADR-GRP-007 — Configuración en tres niveles: formato y precedencia

- **Pregunta** (P8): formato, archivos, estructura, precedencia y validación de la configuración que el motor solo lee.
- **Decisión**: JSON estricto con `$schema` (propuesta base del BRD v0.4). Archivos (PQ-4) `settings.json` (perfil), `.gitraptor/settings.json` (equipo, versionado) y `settings.local.json` (local, en el perfil por ADR-GRP-008). Sección `engine` con niveles admitidos por clave (`x-gitraptor-levels`, generado desde los tipos Rust en `crates/policy`):
  - `baseBranch`: solo equipo, `main`.
  - `idleThresholdMinutes`: perfil y local, 5.
  - `gitPath`: solo perfil (ADR-GRP-009).
  - Intervalos del watcher (sondeo de respaldo y modo degradado): perfil y local, nunca equipo (ADR-GRP-010).
- **Validación (PQ-8)**:
  - JSON inválido, o tipo o rango incorrecto según el schema: se **ignora el nivel entero**, con diagnóstico (archivo y posición, o ruta JSON); los demás niveles aplican.
  - Clave desconocida o clave en un nivel que no la admite: se ignora **solo esa clave**, con diagnóstico; el resto del nivel aplica.
  - Un diagnóstico nunca detiene la observación.
- **Equipo (PQ-9, sustituida por la decisión 1 de Guardrails, 2026-10-04)**: el nivel de equipo se lee de **objetos commiteados**. El **suelo** (blob de la copia de la rama principal, sin `fetch`) es la única fuente de `baseBranch` y de las relajaciones, incluida la clave del mínimo; el `HEAD` del worktree solo endurece (D6). La rama base efectiva es la **confirmada** por el humano (D7, D8); la resuelta distinta es el diagnóstico `base-change-pending`. Lecturas sin objetos de reemplazo.
- **Cargador (enmienda 2026-10-04)**: estado por fuente `ausente` / `legible` / `ignorado` / `parcial`, límites del JSON, `permissions` y `policies` con `x-gitraptor-levels`.
- **Consecuencia ya aplicada**: ADR-GRP-002 (`crates/policy`) y ADR-GRP-004 (formularios) se actualizaron el 2026-10-03 para citar este ADR en lugar de `policy.yaml`.
- **Coautoría**: `permissions`, `policies`, la clave del mínimo y la regla de prohibiciones son de Guardrails (F-001-04). Coautoría cerrada con ADR-GRD-003/004.
- **Impacta**: US-GRP-012, 013, 016. BR-CONS-006, BR-CONS-007, BR-TIME-001. Q23, Q24, Q27, Q36.

### ADR-GRP-008 — Configuración local personal sin versionar

- **Pregunta** (P10): ¿cómo se garantiza que el nivel local no se versione si el motor no escribe en el repo?
- **Decisión (PQ-3)**: `settings.local.json` vive en `<config del perfil>/repos/<id-repo>/`, por repo y no por worktree. El motor solo lo lee, no crea el archivo ni la carpeta, y expone su ruta esperada por el canal. P10 desaparece.
- **Pendiente para el PO**: actualizar la tabla de datos de BR-CONS-001 (fila "configuración local") y BR-CONS-007 a "perfil, indexada por repo".
- **Impacta**: US-GRP-013. BR-CONS-001, BR-CONS-007, BR-TIME-001. Q3, Q23, Q31.

### ADR-GRP-009 — Frontera de solo lectura e invocación del Git del sistema

- **Pregunta**: ¿qué efectos de Git al leer cuentan como escritura, cómo se invoca Git sin efectos y cómo se resuelve Git 2.38 o superior (Q28)?
- **Decisión** (más estricta que la tolerancia de BR-CONS-001): el motor no provoca **ninguna** escritura en el repo ni fuera del perfil, ni siquiera transitoria (un `index.lock` creado y borrado cuenta), y **nunca ejecuta programas configurados por el usuario** (filtros, `textconv`, diff externo, `gpg`, pager, hooks, fsmonitor, credenciales, trace2). gitoxide en solo lectura para el camino caliente; Git CLI solo con una allowlist cerrada de subcomandos de lectura, argv fijo, ruta absoluta, opciones y entorno controlados. `merge-tree --write-tree` prohibido. Resolución: `gitPath` del perfil, PATH heredado y rutas conocidas por SO, sin invocar el shim de macOS sin toolchain.
- **Decisión de producto**: ninguna (frontera técnica delegada por el PO); recomendación aceptada por Rene.
- **Impacta**: US-GRP-001, 002, 003, 012, 014 y todas de forma transversal. BR-CONS-001, BR-AUTH-002, BR-VAL-003, BR-WF-002. NFR-01, NFR-07.

### ADR-GRP-010 — Observación de cambios en worktrees

- **Pregunta**: ¿cómo se detectan los cambios con frescura (NFR-04), a escala (NFR-05) y sin huecos (BR-CONS-005, BR-EDGE-005)?
- **Decisión**: un watcher `notify` compartido (FSEvents, inotify, ReadDirectoryChangesW) sobre working trees y las rutas de `.git` que importan (no `objects/`). Debounce de **ventana fija de 75 ms** por worktree. Recomputo incremental con caché de stat en memoria. Publicación en dos fases. Sondeo de respaldo (30 s) y modo degradado por worktree (2 s), configurables en perfil y local. Reconciliación completa al arrancar, al volver de suspensión, ante desbordamiento y al volver a añadir; lo encontrado queda "sin atribuir" con marca de hueco. Nunca cambia límites del sistema.
- **Enmienda 2026-10-04 (SPIKE-GRP-002)**: debounce con duración efectiva de 75 ms (holgura del temporizador descontada); reconciliación tras cada recreación del stream de FSEvents, con el stream nuevo ya arrancado, y escenario de recreación como gate de CI al subir `notify` (un watcher por worktree en macOS queda como candidata a medir); el sondeo de respaldo solo cubre metadatos de Git, y una reconciliación periódica cada 5 min (⚠️ **ASSUMPTION**) recupera los cambios del working tree perdidos sin marca; ahead/behind en la segunda fase, en proceso con `gix` (⚠️ **ASSUMPTION** sin medir); cachés de `gix` por repo.
- **Decisión de producto**: ninguna; recomendación aceptada por Rene. La valida SPIKE-GRP-002: validado en macOS; Linux y Windows pendientes.
- **Impacta**: US-GRP-002, 003, 004, 005, 006, 014. BR-CONS-005, BR-EDGE-001, BR-EDGE-002, BR-EDGE-005. NFR-04, NFR-05.

### ADR-GRP-011 — Reparto del presupuesto de frescura (NFR-04)

- **Pregunta**: ¿cómo se reparten los 500 ms entre el motor y el Cockpit y cómo se mide?
- **Decisión**: detección ≤ 50 ms, debounce 75 ms, recomputo y persistencia ≤ 150 ms, publicación ≤ 25 ms → **motor ≤ 300 ms**; Cockpit ≤ 100 ms; margen 100 ms que nadie reclama. Tiempos por etapa en cada evento con reloj monótono común. Gate de CI sobre el p95 del total (INF-GRP-002) y aviso por etapa.
- **Enmienda 2026-10-04 (SPIKE-GRP-002)**: el debounce se presupuesta como ventana efectiva de 75 ms; el banco reporta además p99 y máximo; `t0` es el fin del comando en los escenarios de Git; la reconciliación periódica no cuenta para NFR-04; los escenarios de recreación del stream y de reconciliación periódica llevan gate de corrección.
- **Decisión de producto**: ninguna. "< 500 ms" es p95 en las máquinas de referencia: **confirmado en macOS** por SPIKE-GRP-002; ⚠️ **ASSUMPTION** pendiente en Linux y Windows.
- **Impacta**: US-GRP-002. NFR-04, NFR-05. Feature F-001-02.

### ADR-GRP-012 — Detección de sesiones de Claude Code

- **Pregunta**: ¿cómo se detectan sesiones y su estado sin hooks (Q22), sin APIs privadas (NFR-08) y sin atribuir trabajo humano a Claude Code (BR-EDGE-004)?
- **Decisión**:
  - **S1** (proceso y cwd) es necesaria y suficiente para que **exista** la sesión.
  - Un evento solo se **atribuye** con evidencia positiva que apunte a esa sesión: **S2b** (metadatos del transcript), **S3** (ascendencia de procesos), **S4** (hooks de Guardrails, opcional) o el **registro explícito** de un agente sin detección automática ("otro agente"), mientras esa sesión sea la única presente en el worktree. Confirmar una sesión detectada (Q39) no activa esa evidencia.
  - **S2a** (mtime de transcripts) solo correlaciona y desempata; nunca atribuye por sí sola.
  - **La co-ubicación de una sesión detectada nunca basta por sí sola**. En un worktree compartido, evidencia por evento; ante la duda, "sin atribuir".
  - `--resume` o `--continue` es un proceso nuevo y, por tanto, una sesión nueva.
- **Decisión de producto (PQ-2)**: la lectura de `~/.claude/projects` se limita a **metadatos** (herramienta, ruta de archivo, marca de tiempo, id de sesión y cwd). Nunca prompts, respuestas ni código. Adaptador versionado que se desactiva solo si no reconoce el formato. Si SPIKE-GRP-001 fracasa, la vía preferente es **S5** (telemetría OpenTelemetry opt-in) más el refuerzo del registro explícito.
- **Enmienda 2026-10-06 (autoría de commits, BR-26 / US-GRD-018 / US-GRD-019)**: separa **quién ejecutó** (observación) de **a nombre de quién entra** el commit (autor, committer y `Co-Authored-By`), que nunca es evidencia de atribución. La pista `inferred` se valida contra el trailer (`confirmed`, `unconfirmed`; con `contradicted` no se muestra) y no se registra con `human-author`. Los clientes muestran "commit de <persona> con <agente> · <worktree>". Las reglas `agents-commit` y `human-author` se evalúan en la capa de hooks (ADR-GRD-001 a 003), con el actor del hook y sin bloquear ante la duda. Pendiente de ratificar la política; enmiendas de ADR-GRD-003 § 1 y § 4 con la Dev Spec de US-GRD-018. Sin política configurada rige `agents-commit` (BR-AUTH-005). Decisión del orquestador (2026-10-06), validada por el Arquitecto.
- **Impacta**: US-GRP-003, 007, 008, 009. BR-WF-001, BR-TIME-001, BR-EDGE-003, BR-EDGE-004, BR-EDGE-006, BR-AUTH-002. NFR-08. R1, R2, R3, R7, R8.

### ADR-GRP-013 — Modelo persistido de eventos, sesiones y atribución

- **Pregunta**: ¿cómo se persisten eventos, sesiones, registros, confirmaciones (Q39) y correcciones que reatribuyen desde el inicio de la sesión (Q33, Q37), sin emitir nunca "humano" (Q34)?
- **Decisión**: cada evento apunta a una sesión o a ninguna; la atribución efectiva se resuelve desde la sesión con registros append-only (registro, confirmación, corrección, retiro de corrección y retiro de registro). El registro explícito de un "otro agente" atribuye los eventos del worktree mientras sea la única sesión presente; una confirmación no. Secuencia monotónica por repo, eventos inmutables, huecos como intervalos con causa y eventos de reconciliación sin sesión. El contrato no tiene variante "humano".
- **Decisión de producto**: ninguna como ADR. ⚠️ **ASSUMPTION** sobre quién retira un registro (el desarrollador, cualquiera; un agente, solo el suyo). Supuestos dependientes de **P16** (confirmación → origen "registrado") y **P17** (retirar una corrección devuelve los eventos), abiertas para el PO; el modelo admite ambas respuestas.
- **Enmienda 2026-10-06 (autoría declarada)**: los eventos que crean un commit guardan autor, committer y trailers `Co-Authored-By` (con tipo de agente si se reconoce), inmutables y sin el mensaje. Es un dato aparte del actor: no entra en la resolución, la asignación de sesión ni las correcciones. El contrato los expone en un campo opcional, y `inferred` gana su estado frente al trailer. La vista MCP solo lleva el tipo de agente (⚠️ ASSUMPTION). Decisión del orquestador (2026-10-06), validada por el Arquitecto.
- **Impacta**: US-GRP-002, 004 a 011. BR-CONS-002 a 005, BR-EDGE-003, BR-EDGE-005. Q33-Q39, Q41. Contrato de F-001-03 y F-001-04.

### ADR-GRP-014 — Pipeline de release y canales de distribución

- **Pregunta**: ¿cómo se produce y se distribuye el binario único de NFR-06 sin publicar nada sin un paso humano, y con qué nombre y licencia?
- **Decisión**: `release.yml` propio, fijado por SHA y disparado por tag; 6 targets nativos; checksums, SBOM y un borrador de release que solo publica un humano. La firma (Apple, Azure) y las attestations quedan preparadas tras secretos y variables. Los canales se generan en cada release y `release-channels.yml` solo los publica cuando el borrador se publica, con `RELEASE_PUBLISH_CHANNELS` activa y la licencia declarada en todos los manifiestos. Nombre `gitraptor` en los canales (comando `raptor`) y licencia FSL-1.1-ALv2 (§ 6), comprobada por `license.yml`.
- **Decisión de producto**: el nombre y la licencia son decisión de Rene Bonilla (2026-10-05, D4 y D5 del documento de negocio), que cierran la pregunta abierta 2. El resto es decisión del orquestador (2026-10-05), validada por el Arquitecto y el PO. Aceptado el 2026-10-05.
- **Impacta**: INF-GRP-003, INF-GRP-004, TS-GRP-003 (SEC-14 con el canal npm), INF-GRP-002 (medir el binario musl).

## Pendientes fuera de los ADRs

Ver la sección "Pendientes fuera de la arquitectura" del [architecture overview](../architecture-overview.md#10-pendientes-fuera-de-la-arquitectura).
