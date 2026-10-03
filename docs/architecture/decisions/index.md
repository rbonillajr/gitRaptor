---
mode: draft
status: draft
generated: 2026-10-03
generator: architect
domain: GRP
feature: motor-local
total_artifacts: 13
expanded: 4
approved: 4
related:
  context: [CTX-GRP-001]
  rules: [BR-GRP-001]
  adrs: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004]
  stories: [US-GRP-001, US-GRP-002, US-GRP-003, US-GRP-004, US-GRP-005, US-GRP-006, US-GRP-007, US-GRP-008, US-GRP-009, US-GRP-010, US-GRP-011, US-GRP-012, US-GRP-013, US-GRP-014, US-GRP-015, US-GRP-016]
---

# Índice de ADRs — GitRaptor (GRP)

> Outline generado en modo Draft. Los ADR 001 a 004 ya existen y están aceptados. Los ADR 005 a 013 son filas `draft` de la feature `motor-local`: aquí solo se resume la decisión. Expande uno con `/aadd-expand ADR-GRP-0NN`; el cuerpo MADR se escribe en su propio archivo, en la ruta de la columna "Archivo".
>
> **Restricciones activas** (no hay `architecture-constitution.md` en la cascada): ADR-GRP-001 (Rust; gitoxide para leer y Git CLI para escribir; ratatui, clap, rmcp) y ADR-GRP-002 (Nx package-based; `crates/{core,policy,git,api,theme}`, `apps/{cli,mcp}`). ⚠️ **ASSUMPTION**: se tratan como constitución mientras no exista una formal (`/aadd-architect --init-constitution`).

## Outline

| ID | Título | Resumen (1 línea) | Archivo | Status |
|----|--------|-------------------|---------|--------|
| [ADR-GRP-001](./ADR-GRP-001-stack-tecnologico.md) | Stack tecnológico | Decisión: motor, CLI/TUI y MCP en Rust; Tauri + React solo en la Fase 3 | ADR-GRP-001-stack-tecnologico.md | accepted |
| [ADR-GRP-002](./ADR-GRP-002-monorepo-nx.md) | Monorepo Nx package-based | Decisión: pnpm + Cargo workspaces con `@monodon/rust`, crates y apps por fase | ADR-GRP-002-monorepo-nx.md | accepted |
| [ADR-GRP-003](./ADR-GRP-003-design-system.md) | Design system | Decisión: tokens, UI kit y patrones; en el MVP solo tokens y tema de TUI | ADR-GRP-003-design-system.md | accepted |
| [ADR-GRP-004](./ADR-GRP-004-estado-frontend-ux.md) | Estado del frontend y UX | Decisión: estado React de la Fase 3; en el MVP solo los patrones de UX | ADR-GRP-004-estado-frontend-ux.md | accepted |
| ADR-GRP-005 | Forma del motor: proceso por usuario y canal local | Decisión: proceso en segundo plano por usuario con IPC local, para observar sin huecos | ADR-GRP-005-forma-motor-proceso-segundo-plano.md | draft |
| ADR-GRP-006 | Perfil: ubicación por SO, clave de repo y almacenamiento | Decisión: directorios estándar por SO y una base embebida por repo, para separar sin AGPL | ADR-GRP-006-perfil-ubicacion-almacenamiento.md | draft |
| ADR-GRP-007 | Configuración en tres niveles: formato y precedencia | Decisión: JSON con `$schema` y niveles admitidos por valor (P8) | ADR-GRP-007-configuracion-tres-niveles-formato.md | draft |
| ADR-GRP-008 | Configuración local personal sin versionar | Decisión: dónde vive el nivel local para que no se versione nunca (P10) | ADR-GRP-008-configuracion-local-no-versionada.md | draft |
| ADR-GRP-009 | Frontera de solo lectura e invocación del Git del sistema | Decisión: lecturas sin bloqueos opcionales y Git 2.38 o superior resuelto sin efectos secundarios | ADR-GRP-009-frontera-solo-lectura-git.md | draft |
| ADR-GRP-010 | Observación de cambios en worktrees | Decisión: watcher nativo, debounce y reconciliación, para frescura y cero huecos | ADR-GRP-010-observacion-cambios-worktrees.md | draft |
| ADR-GRP-011 | Reparto del presupuesto de frescura | Decisión: motor ≤ 300 ms, Cockpit ≤ 100 ms y 100 ms de margen, medidos por timestamps | ADR-GRP-011-presupuesto-frescura.md | draft |
| ADR-GRP-012 | Detección de sesiones de Claude Code | Decisión: proceso y cwd más señales de actividad; ante la duda, "sin atribuir" | ADR-GRP-012-deteccion-sesiones-claude-code.md | draft |
| ADR-GRP-013 | Modelo persistido de eventos, sesiones y atribución | Decisión: los eventos apuntan a una sesión y la atribución se resuelve desde ella (Q37) | ADR-GRP-013-modelo-eventos-atribucion.md | draft |

## Grafo de dependencias entre ADRs nuevos

| ADR | Depende de | Lo consumen |
|-----|-----------|-------------|
| ADR-GRP-005 | ADR-GRP-006 (dónde viven socket, bloqueo y logs) | 010, 011, 012 |
| ADR-GRP-006 | — | 005, 007, 008, 013 |
| ADR-GRP-007 | 006 (dir de configuración del perfil); context de Guardrails F-001-04 (coautor) | 008, 012 (umbral) |
| ADR-GRP-008 | 007 | — |
| ADR-GRP-009 | — | 010, 012 |
| ADR-GRP-010 | 009, 005 | 011, 012, 013 (huecos) |
| ADR-GRP-011 | 010, 005 | — (Cockpit F-001-02 lo consume) |
| ADR-GRP-012 | 010, 013, 007 (umbral) | — (lo valida SPIKE-GRP-001) |
| ADR-GRP-013 | 006 | 012 |

---

## Resumen de cada ADR nuevo

### ADR-GRP-005 — Forma del motor: proceso por usuario y canal local

- **Pregunta**: ¿cómo observa el motor de forma continua sin una superficie abierta (BR-CONS-005, Q1), cómo arranca con la sesión del SO y cómo se conectan la CLI/TUI y el MCP?
- **Opciones**:
  1. Proceso en segundo plano por usuario (daemon), con autoarranque al iniciar sesión y arranque bajo demanda desde cualquier cliente.
  2. Librería embebida en cada cliente: hay huecos cuando no hay ninguno abierto, los watchers se duplican y varios procesos escriben a la vez en el perfil.
  3. Daemon arrancado solo bajo demanda por los clientes: hay un hueco desde el inicio de sesión hasta que se abre el primer cliente, y lo cubre la reconciliación como "sin atribuir".
  4. Servicio del sistema (root o LocalSystem): privilegios excesivos, varios usuarios y escrituras fuera del perfil. Se descarta.
- **Recomendada**: opción 1. Es la única que cumple "0 huecos mientras la máquina está encendida" (KPI § 3). El arranque bajo demanda es la red de seguridad si el autoarranque falta.
  - **Autoarranque por SO**: LaunchAgent de launchd en macOS, unidad `systemd --user` en Linux, y clave `Run` de HKCU o tarea de inicio de sesión del Programador de tareas en Windows. **Las tres escriben fuera del perfil** (`~/Library/LaunchAgents`, `~/.config/systemd/user`, el registro de HKCU), lo que **choca con Q17**. Ver PQ-1.
  - **Entorno heredado**: launchd y systemd arrancan con un PATH mínimo. La resolución del Git del sistema no puede depender del PATH de la shell (ADR-GRP-009).
  - **Canal**: socket Unix dentro del perfil (archivo 0600 en un directorio 0700) en macOS y Linux; named pipe en Windows con DACL limitada al SID del usuario y que rechaza clientes remotos. Contrato JSON-RPC 2.0 con handshake de versión y un stream de eventos por suscripción, definidos en `crates/api`. Sin puertos TCP (NFR-03).
  - **Instancia única**: archivo de bloqueo en el perfil. Un segundo daemon termina sin observar.
  - **Empaquetado**: modo daemon como subcomando del binario `raptor` (`apps/cli`), sin app nueva, para no tocar ADR-GRP-002. La alternativa `raptord` como app separada exige enmendar ADR-GRP-002. Ver PQ-5.
- **Impacta**: US-GRP-001, 002, 004, 005, 014, 015 y el canal de todas las demás. BR-CONS-005, BR-WF-002, BR-AUTH-001 (Q40: ver PQ-6). Clientes: F-001-02 y F-001-05.
- **Decisión de producto de Rene**: **sí**. Autoarranque frente a Q17 (PQ-1), empaquetado y nombre del daemon (PQ-5), y cómo se distingue al desarrollador de un agente en el canal (PQ-6).

### ADR-GRP-006 — Perfil: ubicación por SO, clave de repo y almacenamiento

- **Pregunta** (P9): ¿dónde vive el perfil en cada SO, cómo se separan los datos de cada repo y en qué formato se guardan?
- **Opciones de ubicación**:
  1. Directorios estándar por SO con el crate `directories`:
     - macOS: `~/Library/Application Support/<app>`.
     - Linux: `$XDG_DATA_HOME`, `$XDG_CONFIG_HOME` y `$XDG_STATE_HOME`.
     - Windows: `%LOCALAPPDATA%`.
  2. Una sola carpeta `~/.gitraptor` en los tres SO, al estilo `~/.claude`: fácil de encontrar y de editar a mano, pero no sigue XDG ni las convenciones de Windows.
  3. `%APPDATA%` (roaming) en Windows. Se descarta: con perfiles móviles de dominio los datos viajarían a otra máquina, en contra de Q31.
- **Opciones de clave de repo**:
  1. Ruta canónica del directorio Git común: se pierde si el repo se mueve.
  2. Id del commit raíz: falla en repos vacíos o con varias raíces, y dos clones del mismo proyecto chocan.
  3. Id generado al añadir el repo, indexado por la ruta canónica del directorio Git común (todos sus worktrees comparten clave), con el commit raíz guardado como pista.
- **Opciones de almacenamiento**:
  1. Base embebida transaccional (SQLite vía `rusqlite`, en modo WAL), un archivo por repo más un índice global.
  2. Archivos append-only (JSONL): la reatribución de Q37 y las consultas a escala obligan a reescribir el archivo o a mantener índices propios.
  3. KV embebido en Rust puro: sin consultas, y cada consulta habría que programarla.
- **Recomendada**:
  - **Ubicación**: opción 1, con `%LOCALAPPDATA%` en Windows y una variable de entorno de sobreescritura para tests con perfiles temporales.
  - **Clave**: opción 3. Retirar y volver a añadir encuentra la misma clave por la ruta (Q25).
  - **Almacenamiento**: opción 1. Un archivo por repo aísla la corrupción (R13): un archivo corrupto se trata como perfil perdido para ese repo (Q26). Permisos 0700 en el directorio y 0600 en los archivos. Licencias: SQLite es de dominio público y `rusqlite` es MIT, aptas según NFR-11.
- **Impacta**: US-GRP-001, 004, 005, 006, 009, 010, 011, 015. BR-CONS-001, BR-CONS-005, BR-AUTH-001, BR-EDGE-005, BR-EDGE-007. Q21, Q25, Q26, Q31.
- **Decisión de producto de Rene**: **sí**, menor. Nombre visible de la carpeta: carpeta estándar por SO o `~/.gitraptor` única (PQ-4). Confirmar que la configuración de perfil tampoco viaja en Windows (Q31, PQ-7).

### ADR-GRP-007 — Configuración en tres niveles: formato y precedencia

- **Pregunta** (P8): ¿formato, nombres de archivo, estructura, precedencia y validación de la configuración que el motor solo lee (Q23, Q24, BR-CONS-007)?
- **Opciones**:
  1. JSON estricto con `$schema` al estilo Claude Code. Es la propuesta de Rene en el changelog v0.4 del BRD:
     - Equipo: `.gitraptor/settings.json`, versionado.
     - Personal: `settings.local.json`.
     - Perfil: `settings.json` en el directorio de configuración del perfil.
     - Secciones `permissions` (allow/ask/deny) y `policies`.
  2. TOML: comentarios e idiomático en Rust, pero sin `$schema` en línea ni el mismo modelo mental que Claude Code.
  3. YAML (el `policy.yaml` original): tipado ambiguo y el crate de serde para YAML ya no tiene mantenimiento.
  4. JSONC o JSON5: admite comentarios, pero el soporte de editores y validadores es desigual.
- **Recomendada**: opción 1.
  - **Esquema**: JSON Schema generado desde los tipos Rust, publicado en el repo e incluido en el binario para validar sin red (NFR-03).
  - **Valores del motor**: en una sección propia (nombre provisional `engine`): `baseBranch`, que solo admite el nivel de equipo y vale `main` por defecto, y el umbral de inactividad, que solo admite perfil y local y vale 5 min por defecto.
  - **Niveles admitidos**: el esquema anota los niveles que admite cada clave (anotación `x-` propia). Dentro de ellos gana el más específico. Un valor en un nivel que no lo admite se ignora con diagnóstico (Q24).
  - **Prohibiciones**: un nivel personal no puede relajar una prohibición del equipo. La regla es de Guardrails.
  - **JSON inválido**: se ignora ese nivel entero, el motor sigue con los demás niveles y los valores por defecto, y expone un diagnóstico con archivo y línea.
  - **Valor de tipo incorrecto**: se ignora esa clave, con diagnóstico.
  - **Claves desconocidas**: se ignoran con aviso, por compatibilidad hacia adelante.
  - **Recarga**: el motor observa los tres archivos (solo lectura) y reaplica los cambios. Ver PQ-8.
  - **Versión de la configuración del equipo**: cada worktree puede tener en su rama una versión distinta de `.gitraptor/settings.json`. Hay que fijar cuál manda para los valores de repo como la rama base. Ver PQ-9.
- **Consecuencia obligatoria al cerrarlo**: actualizar ADR-GRP-002 (línea de `crates/policy` que cita `policy.yaml`) y ADR-GRP-004 (fila "Formularios" que cita `policy.yaml`). No se editan todavía.
- **Coautoría**: las secciones `permissions` y `policies` y la regla de prohibiciones son de Guardrails (F-001-04). El ADR no puede pasar a `accepted` sin el context de Guardrails.
- **Impacta**: US-GRP-013, US-GRP-016 (las dos bloqueadas por P8), US-GRP-012 (valor por defecto `main`). BR-CONS-006, BR-CONS-007, BR-TIME-001. Q23, Q24, Q27, Q36.
- **Decisión de producto de Rene**: **sí**. Nombres de archivo, carpeta y sección (PQ-4), comportamiento ante JSON inválido (PQ-8) y qué versión de la configuración del equipo manda (PQ-9).

### ADR-GRP-008 — Configuración local personal sin versionar

- **Pregunta** (P10): ¿cómo se garantiza que el nivel local personal no se versione, si el motor no escribe en el repo (Q21) y no puede crear un `.gitignore` ni tocar `.git/info/exclude`?
- **Opciones**:
  1. Convención en el repo más un `.gitraptor/.gitignore` versionado junto a la configuración del equipo, que lo crea el usuario o Guardrails, con la entrada `settings.local.json`.
  2. Entrada en `.git/info/exclude` escrita por el comando de Guardrails. No se versiona y es por clon, pero es una ruta operativa que escribe otra feature.
  3. Nivel local guardado en el perfil, fuera del repo, indexado por la clave de repo de ADR-GRP-006. No hay forma de versionarlo; vale por repo y no por worktree (Q3); no viaja (Q31). Contradice la tabla de BR-CONS-001 ("Repo, no versionada").
  4. Detección y aviso: el motor avisa si encuentra `settings.local.json` rastreado por Git. Lo hace en solo lectura y complementa a 1 o 2.
- **Recomendada (técnica)**: opción 3. Elimina P10 de raíz y resuelve que un archivo sin rastrear solo existe en el worktree donde se creó, mientras que el umbral es "por repo".
  - **Si Rene mantiene el archivo en el repo**: opción 1 más la 4, leyendo el nivel local solo del worktree principal.
- **Impacta**: US-GRP-013. BR-CONS-007, BR-CONS-001 (tabla de datos), BR-TIME-001. Q3, Q23, Q31.
- **Decisión de producto de Rene**: **sí**. `settings.local.json` en el repo o en el perfil (PQ-3); la opción 3 obliga al PO a cambiar la tabla de BR-CONS-001.

### ADR-GRP-009 — Frontera de solo lectura e invocación del Git del sistema

- **Pregunta**: ¿qué efectos internos de Git al leer no cuentan como escritura (BR-CONS-001, "ante la duda, es modificación"), cómo se invoca Git sin efectos secundarios y cómo se resuelve y verifica Git 2.38 o superior (Q28)?
- **Opciones**:
  1. Solo Git CLI con lecturas endurecidas: más procesos y más latencia por consulta.
  2. Solo gitoxide: le faltan algunas lecturas de alto nivel, y Git 2.38 se exige igual (Q28).
  3. Mixta: gitoxide en solo lectura para el camino caliente y Git CLI únicamente para una allowlist cerrada de subcomandos de lectura con argv fijo.
- **Recomendada**: opción 3.
  - **Prohibido**: cualquier comando o efecto que escriba refs, índice, configuración, hooks, metadatos de worktrees o archivos de bloqueo. Entre otros:
    - `fetch`, `gc` y `maintenance`, y `worktree prune`/`repair`.
    - El refresco del índice (stat cache) y la escritura de la untracked cache.
    - Arrancar el daemon fsmonitor de Git, que crea archivos en `.git`.
    - `commit-graph write` y `rerere`.
  - **Permitido**: leer objetos, refs, índice y archivos con apertura de solo lectura que permita a otros procesos borrar y renombrar.
  - **Invocación del CLI**:
    - `GIT_OPTIONAL_LOCKS=0` / `--no-optional-locks`.
    - `-c core.fsmonitor=false`, `-c gc.auto=0` y `-c maintenance.auto=false`.
    - Sin prompts de terminal ni credential helpers.
  - **gitoxide**: abierto sin escritura del índice.
  - **Resolución de Git**: no depende del PATH de la shell. Busca en el PATH heredado más las rutas conocidas por SO, y admite una ruta opcional en el perfil. En macOS no invoca el shim `/usr/bin/git` si faltan las Command Line Tools, para no abrir el diálogo de instalación. La versión se vuelve a comprobar de forma periódica en "Esperando Git".
  - **Verificación**: arnés INF-GRP-001. Huella del árbol `.git` y del working tree (lista, contenido y mtime) antes y después, en repos temporales con worktrees, rebase en curso, fsmonitor y untracked cache activados y hooks presentes, en los tres SO.
- **Impacta**: US-GRP-001, 002, 003, 012, 014 y todas de forma transversal. BR-CONS-001, BR-AUTH-002, BR-VAL-003, BR-WF-002. NFR-01, NFR-07. Q10, Q12, Q13, Q21, Q22, Q28.
- **Decisión de producto de Rene**: **no** (frontera técnica que el PO delegó en el Arquitecto).

### ADR-GRP-010 — Observación de cambios en worktrees

- **Pregunta**: ¿cómo se detectan los cambios en los working trees y en `.git` con frescura (NFR-04), a escala (NFR-05) y sin huecos (BR-CONS-005, BR-EDGE-005)?
- **Opciones**:
  1. Watcher nativo con el crate `notify` (FSEvents, inotify, ReadDirectoryChangesW), debounce, recomputo incremental con gitoxide, sondeo ligero de respaldo y reconciliación.
  2. Solo sondeo: es simple, pero 10 o más worktrees por debajo de 500 ms cuestan CPU continua.
  3. fsmonitor de Git o Watchman: el primero escribe en `.git` (lo prohíbe ADR-GRP-009) y el segundo es una dependencia externa que hay que instalar.
  4. Eventos de hooks de Claude Code: prohibido (Q17, Q22).
- **Recomendada**: opción 1.
  - **Qué se observa**: el working tree de cada worktree, más `HEAD`, `refs/`, `packed-refs`, `index`, `logs/`, `worktrees/` y los marcadores de operación en curso del directorio Git común.
  - **Linux**: un watch por directorio, saltando los ignorados. Si se agota el límite de inotify, el worktree pasa a sondeo y el motor expone "observación degradada".
  - **Reconciliación**: completa al arrancar, al volver de suspensión, ante desbordamiento de cola del watcher y al volver a añadir un repo.
  - **Windows**: verificar que los handles del watcher no impiden borrar ni mover un worktree (SPIKE-GRP-002).
- **Impacta**: US-GRP-002, 003, 004, 005, 006, 014. BR-CONS-005, BR-EDGE-001, BR-EDGE-002, BR-EDGE-005. NFR-04, NFR-05.
- **Decisión de producto de Rene**: **no**.

### ADR-GRP-011 — Reparto del presupuesto de frescura (NFR-04)

- **Pregunta**: ¿cómo se reparten los 500 ms de extremo a extremo entre el motor y el Cockpit (F-001-02) y cómo se mide?
- **Opciones**:
  1. Presupuesto fijo por etapa con instrumentación en el evento.
  2. Solo una medición de extremo a extremo, sin reparto: no dice qué etapa se pasó.
  3. Reparto 50/50 sin medición por etapa.
- **Recomendada**: opción 1. ⚠️ **ASSUMPTION**: "< 500 ms" se interpreta como p95 en una máquina de referencia.

  | Etapa | Presupuesto |
  |-------|-------------|
  | Detección SO → motor | ≤ 50 ms |
  | Debounce | ventana de 75 ms |
  | Recomputo incremental y persistencia antes de publicar | ≤ 150 ms |
  | Publicación por IPC | ≤ 25 ms |
  | **Total motor** | **≤ 300 ms** |
  | Cockpit: recepción y render | ≤ 100 ms |
  | Margen | 100 ms |

  - **Medición**: cada evento lleva timestamps de reloj común del sistema por etapa. El banco INF-GRP-002 escribe un archivo en t0 y la TUI sin pantalla registra t_render. El gate de CI falla si se supera el p95.
- **Impacta**: US-GRP-002. NFR-04. Feature F-001-02.
- **Decisión de producto de Rene**: **no** (confirmar la interpretación p95 de la ASSUMPTION).

### ADR-GRP-012 — Detección de sesiones de Claude Code

- **Pregunta**: ¿cómo detecta el motor las sesiones de Claude Code y su estado sin hooks propios (Q22), sin APIs privadas (NFR-08) y sin atribuir nunca a Claude Code el trabajo del editor humano (BR-EDGE-004)?
- **Señales candidatas**:
  - **S1 · Proceso y cwd**: procesos `claude` y su cwd con el crate `sysinfo`. Es información pública del SO; el inicio y el fin del proceso marcan el inicio y el fin de la sesión.
  - **S2 · Transcripts**: archivos de sesión en `~/.claude/projects/<ruta-codificada>/*.jsonl`. El mtime indica actividad. El contenido (herramientas usadas, rutas y horas) permite atribuir por archivo. **Formato no contractual**.
  - **S3 · Ascendencia de procesos**: árbol de procesos y entorno de los `git` invocados, que descienden de `claude` o heredan su marca de entorno. Sirve para atribuir commits y operaciones de Git; es una carrera con procesos cortos.
  - **S4 · Hooks de Guardrails**: señales de esos hooks, si existen (opcional, R8).
  - **S5 · Telemetría local**: OpenTelemetry de Claude Code enviado a un endpoint local. Es documentado, pero lo tiene que configurar el usuario en `~/.claude`.
- **Opciones**:
  1. Solo S1 más co-ubicación: la atribución por cambio es débil porque el SO no informa del proceso que escribió.
  2. S1 + S2 (solo mtime) + S3 + S4.
  3. S1 + S2 (mtime y metadatos de contenido, sin guardar prompts ni código) + S3 + S4, detrás de un adaptador por agente que comprueba la versión del formato y se desactiva solo si no la reconoce.
  4. S5 como señal principal: depende de que el usuario configure Claude Code.
- **Recomendada**: opción 3, condicionada a PQ-2. Sin S2 de contenido, los cambios sin commitear en un worktree con editor humano abierto quedan casi siempre "sin atribuir".
  - **Regla de combinación**: la sesión existe si hay S1 con cwd dentro del worktree. Un cambio o evento se atribuye a Claude Code solo con evidencia positiva (S2 de contenido, S3 o S4). La co-ubicación sola no basta si otro proceso de editor tiene abierto el worktree. Ante la duda, "sin atribuir" (Q34, Q35).
  - **Estados**: activo si hubo actividad dentro del umbral (Q24; 5 min por defecto); inactivo si no; terminado cuando el proceso desaparece, sin reactivarse (Q41).
  - **Prohibido**: instalar hooks de Claude Code en `~/.claude/settings.json` (Q17, Q22).
- **Impacta**: US-GRP-007, 008, 009 (confirmar, Q39), 003. BR-WF-001, BR-TIME-001, BR-EDGE-003, BR-EDGE-004, BR-EDGE-006, BR-AUTH-002. NFR-08. R1, R2, R3, R7, R8. Lo valida SPIKE-GRP-001.
- **Decisión de producto de Rene**: **sí**. Si leer `~/.claude/projects` (formato no contractual) es aceptable bajo NFR-08, y hasta dónde (PQ-2).

### ADR-GRP-013 — Modelo persistido de eventos, sesiones y atribución

- **Pregunta**: ¿cómo se persisten los eventos de Git, las sesiones, los registros, las confirmaciones (Q39) y las correcciones que reemplazan y reatribuyen desde el inicio de la sesión (Q33, Q37), sin emitir nunca "humano" (Q34)?
- **Opciones**:
  1. La atribución se copia en cada evento y una corrección reescribe los eventos afectados: mutación masiva y sin rastro de la corrección.
  2. Cada evento apunta a una sesión, o queda "sin atribuir". La atribución se resuelve desde la cadena de la sesión (detectada, luego corregida o confirmada). Las correcciones son registros append-only, y retirar una restaura la anterior.
  3. Event sourcing completo con proyecciones: más potente, pero desproporcionado para el MVP.
- **Recomendada**: opción 2.
  - **Retirar una corrección**: restaura la atribución anterior, que es el supuesto de P17.
  - **Eventos**: secuencia monotónica por repo y hora UTC con zona horaria. Los eventos nunca se borran.
  - **Huecos**: se guardan como intervalos y generan eventos de reconciliación "sin atribuir" (BR-EDGE-005).
  - **Actor expuesto**: el motor solo expone `agente X + origen` o `sin atribuir`.
- **Impacta**: US-GRP-002, 004, 005, 006, 007, 008, 009, 010, 011. BR-CONS-002, BR-CONS-003, BR-CONS-004, BR-CONS-005, BR-EDGE-003, BR-EDGE-005. Q33-Q39, Q41. Contrato de F-001-03 y F-001-04.
- **Decisión de producto de Rene**: **no** como ADR. Depende de P16 y P17, que ya están abiertas para el PO.
