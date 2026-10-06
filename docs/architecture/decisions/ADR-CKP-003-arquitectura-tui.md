---
id: ADR-CKP-003
title: Arquitectura de la TUI y de la CLI de solo lectura del Cockpit
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-06
deciders: [Orquestador (delegación de Rene Bonilla, 2026-10-04)]
domain: GRP
feature: cockpit
related: [ADR-GRP-001, ADR-GRP-002, ADR-GRP-003, ADR-GRP-004, ADR-GRP-005, ADR-GRP-006, ADR-GRP-009, ADR-GRP-011, ADR-GRP-013, ADR-TMC-004, ADR-TMC-005, ADR-GRD-007, ADR-CKP-001, ADR-CKP-002, TS-GRP-004, INF-GRP-001, INF-GRP-002, CTX-CKP-001, BR-CKP-001, DSYS-GRP-001]
tags: [cockpit, tui, ratatui, crossterm, tea, elm, estado, bucle-de-eventos, render, canal, reconexion, latencia, nfr-04, sec-12, saneado, accesibilidad, i18n, editor, cli]
---

# ADR-CKP-003 — Arquitectura de la TUI y de la CLI de solo lectura del Cockpit

**Status**: Aceptado · **Fecha**: 2026-10-04 · **Decisores**: Orquestador (delegación de Rene Bonilla, 2026-10-04) · **Feature**: Cockpit (F-001-02)

**Decisión del orquestador (2026-10-04), validada por Arquitecto, PO y security-expert** (pasada de endurecimiento del 2026-10-04: ver "Revisión de seguridad (2026-10-04)"). Lo que pide al contrato del canal (E6): N1 a N7 **aplicados el 2026-10-05** (protocolo 6, [Dev Spec de TS-GRP-004](../../requirements/features/motor-local/dev-specs/TS-GRP-004-dev-spec.md) § 7); N11 ya lo cubría TS-CKP-002; N8 a N10 siguen **pendientes, dueño: la historia que los use**.
>
> **Constitución**: no hay `architecture-constitution.md` en la cascada. ⚠️ **ASSUMPTION**: rigen como constitución ADR-GRP-001 (Rust; `ratatui`, `clap`) y ADR-GRP-002 (`apps/cli`, `crates/{api,theme}`), más AGENTS.md (NFR-01, NFR-02). Fuente: inline; se formaliza con `/aadd-architect --init-constitution`.

## Contexto

El Cockpit (F-001-02) es la TUI `raptor`: la cara visible del MVP. Presenta en vivo lo que publica el motor (BR-04 a BR-07) y lanza acciones que ejecuta el daemon. Los ADRs vigentes fijan piezas, pero no la arquitectura interna de la TUI:

| ADR | Qué fija | Qué no cubre |
|---|---|---|
| ADR-GRP-001 | TUI en Rust con `ratatui` | Modelo de estado, bucle, render, backend |
| ADR-GRP-003 / DSYS-GRP-001 | Tokens DTCG → `crates/theme`, widgets, símbolos con fallback, `NO_COLOR`, `--plain`, `insta` | Cómo se resuelven tema y símbolos ni dónde |
| ADR-GRP-004 | Estado React de la Fase 3; en el MVP solo los patrones de UX de su § 3 | Estado de una TUI; saneado SEC-12 (pendiente, overview § 10.3) |
| ADR-GRP-005 | Daemon, canal JSON-RPC, arranque bajo demanda, texto no confiable marcado (§ 5) | Cómo lo consume el cliente: estados de conexión, reconexión |
| ADR-GRP-011 | 100 ms p95 para el Cockpit (`t_client_recv` → `t_render`) | Dónde se toman las marcas y cómo se coalesce el render |
| ADR-GRP-013 | Secuencia monotónica por repo, actor sin variante "humano" | Cómo detecta la TUI huecos y duplicados |

Sin este ADR, cada historia del Cockpit decidiría por su cuenta el estado, el bucle y el saneado, y el presupuesto de 100 ms no tendría un dueño técnico. Restricciones duras (CTX-CKP-001 § 5): la TUI no lee Git, no abre el perfil y no embebe el motor (BR-CKP-CONS-001, Q-CKP-22). Escribe solo con operaciones del catálogo que el daemon ejecuta como operación protegida (ADR-TMC-004, ADR-CKP-002). Nunca hace push. Guardrails decide (BR-CKP-AUTH-001).

**Versiones verificadas (crates.io, 2026-10-04)**: `ratatui` 0.30.2 (2026-06-19, MSRV 1.88) y `crossterm` 0.29.0 (2025-04-05). El toolchain del workspace es 1.99. La 0.30 divide `ratatui` en `ratatui-core`, `ratatui-widgets` y `ratatui-crossterm`. Selecciona la versión de crossterm por feature (`crossterm_0_29`, por defecto la última). Ofrece `ratatui::init()`, `ratatui::restore()` y `ratatui::run()`. `TestBackend` usa `Infallible` como error. Context7 lista la documentación de la 0.29 y la 0.30.0; la API citada sale de las notas de la 0.30 en ratatui.rs.

## Decisión

**Una TUI con arquitectura Elm (TEA): `Msg → update → view`. El estado de vista se deriva solo de la instantánea y de los eventos del motor. Un bucle de un solo hilo dueño del modelo drena colas de entrada y pinta coalescido. Hay un único punto de saneado entre el contrato y el modelo. Todo vive como módulos de `apps/cli`, sobre `crates/api` y `crates/theme`.**

### 1. Stack

- `ratatui` 0.30.x con el backend `crossterm` 0.29 (feature `crossterm_0_29`), fijados como dependencias del workspace y bloqueados por `Cargo.lock`. Los widgets propios se escriben contra los tipos de `ratatui` reexportados; no se depende de `ratatui-core` por separado en el MVP.
- Inicialización y restauración con `ratatui::init()` / `ratatui::restore()`, que instalan el hook de pánico que restaura la terminal. Pantalla alternativa y modo raw solo en la TUI interactiva; nunca en `--plain` ni en la CLI.
- Pruebas de render con `TestBackend` e `insta` (DSYS-GRP-001 § 7).
- **Sin runtime asíncrono en la TUI**: hilos del SO y colas `std::sync::mpsc`. `crates/core` tampoco usa uno hoy. Si la biblioteca cliente de `crates/api` (TS-GRP-004) resulta asíncrona, corre dentro del hilo del canal (§ 3) y la TUI no cambia.
- Linux y Windows (crossterm en consola de Windows, anchura de símbolos, señales). **Pendiente: etapa de validación multiplataforma.**

### 2. Modelo de estado (TEA)

| Pieza | Contenido | Regla |
|---|---|---|
| `Model.engine` (réplica) | Instantánea + eventos aplicados del ámbito global (estado del motor, repos) y del repo seleccionado (worktrees, sesiones, predicción, base, protección, timeline), con la última secuencia aplicada por ámbito | Solo datos publicados. Ningún campo se calcula desde Git ni desde el perfil. Un campo no publicado es "no disponible" (BR-CKP-CALC-001) |
| `Model.ui` | Selección, panel, filtros, layout, modales, toasts, flujos en curso, preferencias | Estado propio de la interfaz; se persiste solo vía daemon (§ 10) |
| `Model.conn` | Estado de la conexión (§ 4) y del arranque del daemon | Visible siempre en la barra de estado |
| `Model.now` | Reloj de la vista, actualizado por `Tick` | `view` nunca lee el reloj: renders deterministas en pruebas |

- **`Msg`**: tecla, pegado, redimensión, mensaje del motor (instantánea, evento, `resync`, respuesta de consulta, resultado de operación), cambio de conexión, `Tick`, vuelta del editor.
- **`update(&mut Model, Msg) -> Vec<Cmd>`** es puro: no hace E/S, no bloquea y no lee el reloj. Devuelve **comandos** (`Cmd`): consulta al daemon, petición de operación del catálogo, guardar preferencias, resolver y abrir el editor, reconectar, salir. Un ejecutor de efectos los corre fuera de `update` y su resultado vuelve como `Msg`.
- **`view(&Model, &mut Frame)`** es puro. Lo derivado de presentación son funciones puras sobre el modelo: orden de la lista (Q-CKP-28), antigüedad relativa (Q-CKP-6, Q-CKP-29), reparto del espacio (§ 7). No es estado propio.
- **Sin UI optimista en escrituras**: una acción muestra al instante su estado pendiente (feedback < 100 ms). El resultado solo se pinta cuando llega el evento del motor o la respuesta del ejecutor. Es lo contrario de `useOptimistic` (ADR-GRP-004 § 2): en el Cockpit la fuente única manda (BR-CKP-CONS-001).
- **Flujos críticos como máquinas de estado explícitas** (enums de Rust en `Model.ui`): merge, rebase, descartar, crear worktree, excepción consciente con su ventana (ADR-GRD-007) y confirmación de trabajo ajeno (ADR-TMC-005). Ejemplo: `Inactivo → Confirmando → Pedida(id) → Anunciada(cuenta atrás) → Ejecutando → Hecha(deshacer) | Rechazada(motivo) | Detenida(conflicto)`. Es la regla "sin estados imposibles" de ADR-GRP-004 § 1, adaptada a Rust.
- **Escrituras solo con datos frescos**: con la conexión fuera de "En vivo", las acciones de escritura se desactivan con su motivo. Cada petición lleva el estado esperado para que el ejecutor revalide (BR-CKP-CONS-004, ADR-CKP-002).
- **Acciones según la capa** (M-03): el daemon fija la capa según el solicitante (ADR-CKP-002 § 4). Si la TUI la abre un agente, su capa es `mcp`: integrar, descartar, Cancelar, la excepción y la salida de Git se desactivan con su motivo, y el rebase usa el modo `atomic`. La TUI lo sabe por el handshake (N5) y nunca lo decide ella.

### 3. Bucle de eventos y render coalescido

- **Hilo principal**: único dueño de `Model` y de la terminal. Ejecuta `update` y `view`; nunca bloquea en E/S.
- **Hilo de entrada**: lee teclado, pegado y redimensión de crossterm con sondeo de plazo corto, para poder pausarse (§ 9), y los envía a la **cola de entrada**.
- **Hilo del canal**: conexión, handshake, suscripción, consultas y peticiones al daemon (§ 4). Envía mensajes del motor a la **cola del motor** y estampa `t_client_recv` en cada uno.
- **Ticks**: no hay hilo. El hilo principal espera con `recv_timeout` hasta el siguiente vencimiento: antigüedad y cuentas atrás a 1 Hz, expiración de toasts a 5 s (DSYS-GRP-001 § 3). Un tick solo marca la vista como sucia si cambia un texto visible.
- **Iteración**: (1) drenar **toda** la cola de entrada; (2) drenar la cola del motor hasta vaciarla o agotar un presupuesto de aplicación (⚠️ **ASSUMPTION**: 8 ms; lo fija el banco de Validación V3); (3) si el modelo quedó sucio, **un solo `draw`**; (4) estampar `t_render` en los mensajes aplicados en esa iteración.
- **Coalescencia**: no hay frecuencia fija de frames. El tiempo de pintado actúa como ventana: lo que llega mientras se pinta se aplica junto en la siguiente iteración. Una ráfaga de 1.000 archivos produce pocos frames, no 1.000. La entrada se drena primero, así que una tecla nunca espera detrás de una ráfaga del motor más que el presupuesto de aplicación.
- **Feedback < 100 ms por tecla** (ADR-GRP-004 § 3): toda tecla produce un cambio visible en la siguiente iteración (selección, estado pendiente o aviso de acción desactivada con su motivo). Nada que dependa del daemon se espera en el hilo principal.

### 4. Cliente del canal (consumo de DEP-CKP-6)

La TUI y la CLI usan la biblioteca cliente de `crates/api` (TS-GRP-004). Encima de ella, el módulo `client` de `apps/cli` implementa:

- **Comprobación del par antes del handshake** (L-06): el cliente comprueba que el directorio del socket es del uid y tiene permisos 0700, y que el uid del par es el propio (`getpeereid` en macOS, `SO_PEERCRED` en Linux). Si falla, "Canal rechazado" sin enviar nada. **Enmienda (2026-10-05, TS-GRP-004)**: vive en la biblioteca cliente (`channel::transport::connect`), que comprueba la carpeta (0700, del usuario, sin enlace simbólico) y el uid del servidor, y devuelve `ChannelRejected` sin enviar nada. La biblioteca está hoy en `crates/core`: sacarla de ahí para cumplir § 5 y V5 es **pendiente, dueño: INF-CKP-001**. Windows: **Pendiente: etapa de validación multiplataforma.**
- **Arranque coherente por ámbito** (global y repo seleccionado): instantánea con secuencia `N`, suscripción desde `N+1`. Los eventos con secuencia `≤ última aplicada` se descartan (duplicados). El evento `última + 1` se aplica. Uno mayor es un **hueco**: no se aplica nada más y se pide una nueva instantánea ("Resincronizando").
- **`resync` del daemon** (SEC-08, cliente lento): mismo camino que un hueco. Mientras dura, la vista conserva lo último aplicado, marcado como desactualizado.
- **Reconexión**: al perder el canal, la réplica se conserva marcada "desconectado desde hh:mm" y nunca se presenta como actual. El cliente reintenta con espera exponencial (⚠️ **ASSUMPTION**: de 250 ms a un máximo de 5 s, sin límite de intentos mientras la TUI esté abierta). Cada reconexión rehace handshake e instantánea; el MVP no reanuda desde una secuencia.
- **Cambio de repo** (Q-CKP-1): baja de la suscripción del repo anterior, instantánea y suscripción del nuevo.
- **Enmienda (2026-10-06, US-CKP-001): sesiones del repo.** La instantánea del ámbito repo (N1) trae los worktrees, pero no las sesiones de agente. Tras la instantánea y la suscripción del repo, el hilo del canal pide `sessions.list {repo_id, include_ended: false}` y lo entrega a la cola del motor. `update` fusiona esa lista y cada `session.state` con un único upsert por sesión: gana el `state_since` posterior y, si empatan, la Terminada. Converge en cualquier orden, porque la lista se pide después de suscribirse desde `N+1`. Cada instantánea nueva (hueco, `resync`, reconexión, cambio de repo) vacía las sesiones y las vuelve a pedir. Sin la lista, o con `detection_available: false`, el agente es "no disponible". **Decisión del orquestador (2026-10-06), validada por Arquitecto.** Deuda: incluir las sesiones en la instantánea del repo cuando se toque el contrato, **dueño: TS-GRP-004**.

| Estado de conexión | Qué ve el desarrollador (BR-CKP-WF-004) | Escrituras |
|---|---|---|
| Conectando | Indicador en la barra de estado | No |
| Arrancando el motor | "Arrancando el motor…" (§ 5) | No |
| Sincronizando | Esqueleto de la vista | No |
| En vivo | Vista normal con los estados del motor (Esperando Git, Sin repos, Observando, Reconciliando, Degradada) | Sí, con sus precondiciones |
| Resincronizando | Datos marcados como desactualizados | No |
| Reconectando | "Desconectado desde hh:mm", intento N | No |
| Motor no disponible | Qué pasó y cómo arrancarlo (`raptor daemon`, `raptor daemon enable`); tecla de reintento | No |
| Versión incompatible | La biblioteca cliente lo resuelve si es el binario instalado (ADR-GRP-005 § 4); si no, instrucciones | No |
| Canal rechazado | Permisos del socket o del pipe alterados (SEC-01); qué revisar | No |

**Necesidades del contrato del canal**. **Enmienda (2026-10-05, TS-GRP-004)**: N1 a N7 aplicados en el protocolo 6. N11 ya lo cubre TS-CKP-002 (huella del plan y revalidación). N8, N9 y N10 siguen pendientes, dueño: la historia que los use:

| # | Necesidad | Origen |
|---|---|---|
| N1 | Instantánea con secuencia `N` y suscripción desde `N+1`, sin hueco ni duplicado, por ámbito (global y por repo) | DEP-CKP-6 |
| N2 | Secuencia contigua por ámbito en el stream y evento `resync` explícito con su causa | SEC-08, § 4 |
| N3 | Ámbito global: estado del motor (BR-WF-002), repos observados con un resumen de atención por repo (⚡, ⛔, hueco) para el selector, y si el autoarranque está registrado (aviso de ADR-GRP-005 § 3) | Q-CKP-1, BR-CKP-WF-004 |
| N4 | Consulta "repo de esta ruta": el daemon canonicaliza y devuelve el id del repo observado, porque la TUI no lee Git | Q-CKP-1, BR-VAL-002 |
| N5 | Solicitante y capa resueltos de la conexión en el handshake, para que la vista diga "actúas como X" y desactive lo que la capa no permite. Main ya expone `requester.resolve` (Dev Spec de TS-TMC-004 § 2), que puede cubrirla; es solo UX, porque el daemon re-resuelve en cada petición (ADR-CKP-002 § 3) | Q-CKP-16, ADR-TMC-005 § 1, M-03 |
| N6 | Texto no confiable con un **tipo propio** en el contrato (envoltorio), con longitud máxima por campo, para imponer el saneado por tipo (§ 8) | SEC-12, ADR-GRP-005 § 5 |
| N7 | Estados, diagnósticos y motivos como códigos tipados con parámetros, sin cadenas de presentación | NFR-10 |
| N8 | Consultas bajo demanda (diff, grafo, timeline) con id de petición, cancelación y topes, respondidas fuera del orden del stream sin bloquearlo | DEP-CKP-2, DEP-CKP-3, DEP-CKP-5 |
| N9 | Lectura y escritura de las preferencias de la TUI, como documento acotado y versionado. **La escritura solo se acepta de un "sin atribuir" que pasa los controles 1 a 3** de ADR-GRP-005 § 6; el esquema excluye `cockpit.editor`, `cockpit.editorKind` y `cockpit.worktreePathTemplate` | DEP-CKP-11, L-05 |
| N10 | Resolución del editor: el daemon devuelve el argv validado de la configuración y la ruta destino absoluta y validada (§ 9) | Q-CKP-9, DEP-CKP-13, I-03 |
| N11 | Peticiones de operación con el estado esperado para revalidar | BR-CKP-CONS-004, ADR-CKP-002 |

### 5. Arranque del daemon (Q-CKP-22)

- Si no puede conectar, el cliente pide el arranque a la biblioteca de `crates/api` (gestor de servicios o entorno limpio por allowlist, ADR-GRP-005 § 3, SEC-10) y espera el handshake con un tiempo máximo (⚠️ **ASSUMPTION**: 5 s). Si falla, pasa a "Motor no disponible" con instrucciones.
- **La TUI nunca embebe el motor**. El binario `raptor` contiene el motor porque `raptor daemon` es un subcomando (ADR-GRP-005 § 1). Por eso la frontera es de módulo: **solo** el módulo `daemon` de `apps/cli` importa `gitraptor_core`. Un chequeo estático en CI lo hace cumplir (Validación V5). Los módulos de la TUI y de la CLI no importan `gitraptor_core`, `gitraptor_git` ni `gitraptor_policy`.
- Sin autoarranque registrado, la TUI avisa una vez por sesión, sin activarlo (ADR-GRP-005 § 3).

### 6. Presupuesto de 100 ms p95 e instrumentación (ADR-GRP-011)

- **`t_client_recv`**: lo toma el hilo del canal al terminar de leer del transporte el mensaje completo, **antes** de deserializarlo. Decodificar cuenta en el presupuesto del Cockpit.
- **`t_render`**: lo toma el hilo principal al volver `Terminal::draw` del primer frame dibujado después de aplicar el mensaje. El buffer ya se volcó al backend.
- Ambos con el helper de reloj monótono de `crates/api` (ADR-GRP-011 § 3). Nunca se devuelven al motor y nunca salen de la máquina (NFR-03).
- **Histograma local** en memoria por etapa (decodificar, aplicar, pintar) y total. Se ve en un panel de diagnóstico de la TUI y, con `--timings` (nombre provisional), se imprime al salir. La TUI no escribe el perfil.
- **Gate de CI sin pantalla**: el arnés de INF-GRP-002 conduce la misma `App` (colas, `update`, `view`) sobre `TestBackend`, conectada al daemon real del banco. Gates en Validación V3.
- **Enmienda (2026-10-06, US-CKP-001)**: el gate con el daemon real es el escenario `tui-modify` del banco INF-GRP-002. Mide "modificar un archivo" de punta a punta, de `t0` al `t_render` del frame que muestra el cambio (NFR-04: 500 ms p95, más el exceso de holgura del temporizador), y la etapa propia de la TUI (100 ms p95). Los 100 ms fallan en todos los modos: son trabajo de CPU sobre `TestBackend`. Los 500 ms fallan en `reference` y en `ci` solo se reportan; allí bloquea el techo de regresión, confirmado 2 de 3. `App` expone `metrics.last_render_ns` como marca final. **Decisión del orquestador (2026-10-06), validada por Arquitecto.**

### 7. Layout 80×24 y prioridades (Q-CKP-18, BR-CKP-EDGE-005)

- Función pura `layout(area, &Model) -> Regiones`: cabecera de una línea (repo, motor, conexión, protección, solicitante, y el recuento de ⚡ y ⛔), cuerpo y `KeyHints` de una línea.
- **Los filtros nunca ocultan** el recuento de ⚡ y ⛔ de la cabecera (L-05): una preferencia escrita por otro no puede esconder una alerta.
- El cuerpo se reparte por prioridad: **lista > alertas (⚡, ⛔) > grafo > detalle**. El grafo colapsa primero. El detalle pasa a vista a pantalla completa bajo demanda. Las cifras por región son de la Dev Spec.
- Por debajo de 80×24 solo se pinta el mensaje de tamaño mínimo. Superposiciones modales (ConfirmPrompt, PolicyBanner, ayuda `?`) sobre cualquier región.
- La anchura de cada símbolo se toma del tema activo (§ 10), no del texto, porque `⚡` y `⛔` ocupan dos columnas en muchas terminales y sus fallbacks ASCII ocupan más.

### 8. Saneado SEC-12 en un único punto (DEP-CKP-9)

- **Punto único**: el adaptador de ingesta `present::ingest` convierte los tipos del contrato en el modelo de vista. Es el único sitio donde un texto no confiable (N6) pasa a `SafeText`, mediante `present::sanitize`.
- `SafeText` tiene **constructor privado** de ese módulo. Los widgets solo aceptan `SafeText` o texto del catálogo i18n. El modelo nunca guarda texto no confiable en bruto. Las acciones referencian ids, no textos, así que no hace falta devolver el original al daemon.
- **Reglas del saneado**:
  - C0, DEL y C1 (U+0080 a U+009F, incluido CSI U+009B) se vuelven visibles como escapes (`\x1b`, `\u{9b}`), nunca se emiten ni se borran en silencio. Así una secuencia OSC 52 o un cambio de título se ve y no actúa (BR-CKP-VAL-002).
  - Los controles bidi (U+061C, U+202A a U+202E, U+2066 a U+2069), los de anchura cero e invisibles (U+200B a U+200F, U+2060 a U+2064, U+FEFF) y la tabla de Tags (U+E0000 a U+E007F) también se hacen visibles (L-03).
  - Saltos de línea, tabuladores y los separadores U+2028 y U+2029 en campos de una línea se sustituyen por un escape visible (L-03).
  - Recorte por anchura de visualización con elipsis, tope de longitud por campo y de marcas combinantes por grafema. Los **nombres** (ramas, worktrees, agentes, etiquetas) llevan un tope de 100 caracteres (L-03).
- La CLI humana (`raptor status`, `raptor conflicts`) usa la misma ingesta. La salida `--json` **no pinta**: serializa con un escapador propio que emite como `\uXXXX` (o con su par sustituto) los C0, C1, DEL, bidi, anchura cero, U+2028, U+2029 y Tags. El dato llega completo y no es ejecutable en una terminal. `serde_json` solo escapa los C0.
- **Las mismas categorías rigen las respuestas del MCP** (ADR-CKP-002 § 12, SEC-12). Este ADR no fija el código del MCP, pero la lista de categorías es una sola.
- **Defensa en profundidad**: el daemon ya marca el texto como no confiable (ADR-GRP-005 § 5). El cliente sanea de todos modos y no confía en el daemon para ello.

### 9. Editor de terminal y suspensión (Q-CKP-9)

- La TUI pide al daemon la resolución del editor (N10): argv de la configuración del perfil o local personal, validado sin shell. Sin configuración, usa su propio `$VISUAL`/`$EDITOR` con la misma validación: metacaracteres de shell rechazados con motivo (BR-CKP-VAL-003). La función de validación es pura y vive en `crates/api`, compartida con el daemon.
- **Revalidación** (L-06): la TUI vuelve a pasar el argv que recibe del daemon por la misma función pura antes de lanzarlo. Si no pasa, no lanza nada y muestra el motivo.
- **Argv visible y aprobado** (L-05): la TUI muestra el argv que va a lanzar. Si cambió respecto del último aprobado, pide confirmación (default No) antes de lanzar. La huella del último argv aprobado se guarda en las preferencias (N9), que solo escribe un "sin atribuir" que pasa los controles 1 a 3; así un agente que reescribe `cockpit.editor` no lanza nada sin que el humano lo vea.
- **Ruta destino absoluta** (I-03): el editor recibe siempre la ruta absoluta que validó el daemon (N10), nunca una relativa al cwd de la TUI.
- **La TUI lanza el proceso**, porque es quien tiene la terminal. Lo hace con argv fijo y sin shell, desde el módulo autorizado `tui::editor` (DEP-CKP-12).
- **Editor de terminal**:
  1. El hilo principal pausa el hilo de entrada y espera su confirmación, para que no robe teclas al editor.
  2. Restaura la terminal: sale de la pantalla alternativa y del modo raw.
  3. Un hilo de efectos lanza el editor con la entrada y la salida heredadas y espera.
  4. Mientras espera, el bucle sigue aplicando eventos del motor **sin pintar**.
  5. Al volver: reinicializa la terminal, reanuda la entrada, fuerza un redibujado completo y emite `Msg` con el resultado.

  El mismo mecanismo sirve para `Ctrl-Z`.
- **Editor gráfico**: se lanza sin esperar, con la entrada y la salida nulas, y no bloquea. ⚠️ **ASSUMPTION**: la TUI clasifica el editor como de terminal con una lista conocida (`vi`, `vim`, `nvim`, `nano`, `emacs -nw`, `hx`, `micro`, `kak`…). Una clave de configuración del editor permite forzar la clasificación (va con DEP-CKP-13). Si no se reconoce, se trata como de terminal: suspender es seguro, y lanzar sin esperar un editor de terminal no lo es.
- Sin editor: error accionable (BR-CKP-EDGE-007). Windows y Linux: **Pendiente: etapa de validación multiplataforma.**

### 10. Tema, accesibilidad e i18n

- **Tokens**: `packages/design-tokens` (DTCG) → Style Dictionary → `crates/theme` (ADR-GRP-003). `crates/theme` sigue **agnóstico de `ratatui`**: expone por token semántico truecolor, índice de 256 colores y fallback de 16 colores, y por símbolo su glifo, su fallback ASCII y su anchura. `apps/cli` lo mapea a `ratatui::style`. Ningún widget usa colores ni glifos literales.
- **Resolución del tema**: `--no-color` o `NO_COLOR` no vacío → sin color, solo atributos (negrita, inverso). `--theme high-contrast` → alto contraste. Si no, la profundidad se detecta con `COLORTERM` y `TERM`. (Enmienda 2026-10-05, TS-CKP-004: el juego normal tiene variante para terminal oscura y clara. `--theme light|dark|high-contrast|auto` > `GITRAPTOR_THEME` > OSC 11 > `COLORFGBG` > oscura; sin color no se consulta la terminal. La lógica está en `crates/theme` y la consulta, en `apps/cli` `term`, antes del lector de eventos. Ver la [Dev Spec de TS-CKP-004](../../requirements/features/cockpit/dev-specs/TS-CKP-004-tokens-semanticos-simbolos.md) § 8.)
- **Símbolos**: `--ascii`, o una locale que no sea UTF-8 → fallback ASCII. El color nunca va solo (NFR-09). A partir del noveno agente, los colores `agent.n` se reutilizan y el nombre y el símbolo distinguen (BR-CKP-EDGE-006).
- **Teclado primero**: `tui::keymap` es la tabla única acción ↔ teclas. Alimenta a la vez la interpretación de la entrada, `KeyHints` y la ayuda `?`, así que no pueden divergir. ⚠️ **ASSUMPTION**: la captura del ratón está desactivada por defecto, para no romper la selección de texto de la terminal.
- **`--plain`** (DSYS-GRP-001 § 6): mismo `Model` y `update`, otro renderer (`tui::plain`). Sin pantalla alternativa ni movimiento del cursor: escribe la vista inicial como texto y después **solo líneas nuevas** con los cambios y las alertas. En el MVP, `--plain` es de lectura y alertas, y las acciones de BR-07 requieren la TUI completa. Lo validó el PO como riesgo **R-CKP-10** de CTX-CKP-001 (decisión del orquestador, 2026-10-04, validada por PO): la ayuda `?` y la de `--plain` lo dicen, y las acciones en `--plain` quedan como candidato post-MVP.
- **i18n en/es** (NFR-10): **catálogo tipado** en `present::i18n`. Una enumeración de mensajes con un `match` exhaustivo por idioma, así que una traducción ausente es un error de compilación. Los parámetros son `SafeText` o números, sin concatenar cadenas. Idioma: `--lang`, después `LC_ALL`, `LC_MESSAGES`, `LANG`, y por último `en`. Los códigos del motor (N7) se traducen aquí. `--json` nunca se localiza.
- **Preferencias** (Q-CKP-17, BR-CKP-CONS-006, DEP-CKP-11): repo, panel, filtros y layout. Se cargan al conectar (N9) y se guardan vía daemon con un rebote de 1 s y al salir (⚠️ **ASSUMPTION**). **Solo se guardan si el solicitante es un "sin atribuir" que pasa los controles 1 a 3** (L-05); si no, la TUI las mantiene en memoria y lo dice una vez. El esquema no admite las claves del editor ni la plantilla de worktrees. Con varias TUIs, gana la última escritura. Un fallo al guardar se avisa sin bloquear. Al arrancar se usa el repo del directorio actual (N4) o, si no, el último usado (Q-CKP-1). Los flags de la línea de comandos mandan sobre las preferencias.

### 11. CLI de solo lectura (Q-CKP-20, BR-CKP-CONS-007)

- `raptor status` y `raptor conflicts` reutilizan `client` (una instantánea, sin suscripción) y `present::ingest`. Ven por tanto el mismo estado que la TUI. Arrancan el daemon igual que ella (§ 5).
- Salida humana con el renderer de texto, color solo en TTY (DSYS-GRP-001 § 4). `--json` usa un **esquema propio de `apps/cli`, versionado**, mapeado desde el modelo de vista, y no reenvía el contrato de `crates/api`. Así la allowlist (sin mensajes de commit ni contenido de diff) se impone en un solo mapeo, y el contrato interno puede evolucionar sin romper scripts.
- ⚠️ **ASSUMPTION**: los códigos de salida siguen DSYS-GRP-001 § 4. `raptor conflicts` devuelve `4` si hay al menos un ⚡ vigente, para scripts; se confirma en la Dev Spec.
- `raptor` sin subcomando abre la TUI si la entrada y la salida estándar son TTY. Si no, termina con código `2` y sugiere `raptor status`. **Enmienda (2026-10-05, INF-CKP-001)**: `raptor tui` es un alias explícito con el mismo comportamiento (decisión del orquestador, validada por Arquitecto).

### 12. Ubicación en el monorepo

Módulos de `apps/cli`, sin crates nuevos: ADR-GRP-002 no cambia. `apps/cli` añade un **target de biblioteca interno** del mismo paquete para que las pruebas y el arnés de INF-GRP-002 conduzcan la `App` sin pantalla.

| Módulo | Responsabilidad | Depende de |
|---|---|---|
| `daemon` | Entrada del subcomando `raptor daemon`; **único** que importa `gitraptor_core` | `crates/core` |
| `client` | Conexión, estados (§ 4), secuencia por ámbito, resync, reconexión, arranque bajo demanda delegado | `crates/api` |
| `model` | `Model`, `Msg`, `Cmd`, `SafeText` (tipo) | — |
| `present` | `ingest` (punto único de saneado), `sanitize`, `i18n`, `json` | `crates/api` |
| `tui` | `app` (bucle y colas), `update`, `effects`, `view` (layout y widgets de DSYS-GRP-001 § 3), `keymap`, `term` (init, restore, suspensión), `editor`, `plain`, `metrics` | `ratatui`, `crossterm`, `crates/theme` |
| `cli` | `status`, `conflicts` | `client`, `present` |

En el daemon, el **publicador de la predicción** (ADR-CKP-001) y el **ejecutor de operaciones** (ADR-CKP-002) son de este feature, pero no de este ADR. La TUI solo los consume por el canal. Vista de componentes: [c4-ckp-components.md](../diagrams/c4-ckp-components.md).

## Alternativas consideradas

| Eje | Alternativa | Por qué no |
|---|---|---|
| Estado | Componentes con estado propio (cada panel guarda y pide sus datos) | Varios dueños del mismo dato y consultas dispersas. El render depende del orden de llegada. Difícil de probar sin terminal |
| Estado | Réplica compartida (`Arc<Mutex<_>>`) que escribe el hilo del canal, con render por temporizador | Carreras entre aplicar y pintar. El temporizador añade latencia fija y frames vacíos |
| Estado | **TEA, `update` y `view` puros (elegida)** | Un dueño del estado. Pruebas deterministas de lógica (`update`) y de render (snapshots). Encaja con "cada dato, una sola capa dueña" (ADR-GRP-004 § 1) |
| Runtime | `tokio` con `EventStream` de crossterm (feature `event-stream`) | Un runtime asíncrono que nada más usa hoy. La ganancia es nula con tres fuentes de eventos |
| Render | Frecuencia fija (p. ej. 30 fps) | Hasta 33 ms de latencia añadida y frames sin cambios |
| Render | Pintar por mensaje | Una ráfaga del motor produce cientos de frames y la entrada se queda atrás |
| Backend | `termion` / `termwiz` | `termion` no funciona en Windows (NFR-06). `crossterm` es el backend por defecto de `ratatui` |
| Saneado | En cada widget | Muchos puntos que olvidar. Un widget nuevo podría pintar texto en bruto |
| Saneado | Solo en el daemon | Rompe la defensa en profundidad. ADR-GRP-005 § 5 asigna la limpieza a los clientes |
| Ubicación | Crate `crates/tui` | Exige enmendar ADR-GRP-002 sin beneficio: nada fuera de `apps/cli` reutiliza la TUI (la Fase 3 es React) |
| i18n | Fluent / `rust-i18n` con archivos de recursos | Una clave ausente se descubre en ejecución. El catálogo tipado la descubre al compilar, que es el guardrail que busca ADR-GRP-001 para el código escrito por agentes. Se revisa si hace falta pluralización compleja |

## Consecuencias

- ✅ El presupuesto de 100 ms tiene dueño, marcas definidas y gate. Un fallo dice si se pasó decodificar, aplicar o pintar.
- ✅ La fuente única se cumple por construcción: el modelo solo se llena desde la ingesta del contrato y la TUI no importa el motor.
- ✅ SEC-12 se impone por tipo: un widget no compila si recibe texto no confiable.
- ✅ La CLI de solo lectura y la TUI no pueden divergir: comparten cliente, ingesta y catálogo.
- ⚠️ `TestBackend` no mide el volcado a una terminal real. **Mitigación**: el histograma local mide el pintado real en dogfooding. Una variante del banco sobre una pseudo-terminal queda para la Dev Spec de INF-GRP-002.
- ⚠️ La anchura de `⚡`, `⛔` y `⚠` varía entre terminales. **Mitigación**: anchura declarada en el tema, `--ascii` y snapshots con ambos juegos de símbolos.
- ⚠️ Depende de N1 a N11 del canal. **Mitigación**: el orden de entrega de Q-CKP-24 empieza por BR-04, que solo necesita N1 a N7. Lo que falte se presenta como "no disponible" (BR-CKP-CALC-001).
- ⚠️ `crates/theme` y `packages/design-tokens` hoy solo tienen dos primitivos. Los tokens semánticos, los símbolos con fallback y la anchura son un **requisito previo** de cualquier pantalla.
- ⚠️ `--plain` sin acciones deja a un usuario de lector de pantalla sin BR-07 en el MVP. Es el riesgo R-CKP-10, aceptado por el PO (§ 10).
- ⚠️ Una TUI abierta por un agente funciona con capa `mcp` y sin varias acciones (M-03). Es lo buscado: un agente no gana poder cambiando de cliente.
- ⚠️ El catálogo tipado obliga a recompilar para corregir un texto. Se acepta: los textos viajan con el binario.
- ⚠️ La clasificación del editor de terminal por lista conocida puede fallar. **Mitigación**: lo desconocido se suspende (seguro) y hay una clave para forzar la clasificación.

## Validación

Las pruebas usan repos y perfiles temporales y pasan por el arnés de INF-GRP-001 (repo intacto). Nunca usan el repo de GitRaptor.

1. **V1 · Snapshots de render** (`insta` sobre `TestBackend`) con reloj fijo.
   - Tamaños: 80×24, 100×30, 120×40 y 79×24.
   - Temas: truecolor, 256 colores, 16 colores, `NO_COLOR` y alto contraste.
   - Símbolos Unicode y ASCII; idiomas en y es.
   - Estados: Sin repos, Esperando Git, Reconciliando, Degradada, Reconectando, Motor no disponible, 10 worktrees con 55 pares, ⚡ nuevo con ConflictAlert y toast, PolicyBanner, ConfirmPrompt, más de 8 agentes, worktree compartido, "actúas como claude-1" y rama maliciosa.
2. **V2 · `update`**: pruebas de propiedades sobre la secuencia (duplicado descartado; hueco → resincronización sin aplicar nada después; `resync`; reconexión). Orden Q-CKP-28. Transiciones de cada máquina de flujo. Ninguna escritura cambia la réplica sin evento del motor.
3. **V3 · Latencia sin pantalla** (banco INF-GRP-002 con el daemon real y la `App` sobre `TestBackend`):
   - Gates ya fijados por ADR-GRP-011: extremo a extremo p95 ≥ 500 ms **falla**; etapa por encima de su presupuesto con el total dentro, **aviso**.
   - Gate propuesto (enmienda E2): p95 del Cockpit (`t_client_recv` → `t_render`) > 100 ms **falla**.
   - Feedback por tecla (tecla leída → frame) p95 ≥ 100 ms: **aviso**.
   - Microbanco sintético sin daemon en cada PR que toque `apps/cli`: ráfaga de 1.000 archivos, 10 worktrees y 55 pares.
4. **V4 · Saneado**: fuzzing y propiedades. La salida de `sanitize` y del escapador JSON no contiene C0, C1, DEL, bidi (incluido U+061C), anchura cero (incluidos U+2060 a U+2064), U+2028, U+2029 ni caracteres de la tabla de Tags. El corpus incluye `\x1b]52;…`, `\x1b]0;…`, `\x1b[2J`, U+009B, RLO, U+061C, U+2028, U+2063 y U+E0041. Un nombre de 300 caracteres sale recortado a 100 (L-03). Snapshot de una rama maliciosa: secuencia visible y buffer sin ESC (ADR-GRP-005 Validación 12, SEC-12).
5. **V5 · Fitness estáticas en CI**: `gitraptor_core` solo en el módulo `daemon`. Ni `gitraptor_git` ni `gitraptor_policy` en `apps/cli`. Lanzar procesos solo en `tui::editor` y en la biblioteca cliente de `crates/api` (DEP-CKP-12). Ningún widget recibe `String` del contrato (por tipo).
6. **V6 · Sin daemon**:
   - Con el daemon parado, la TUI llega a "En vivo".
   - Con el arranque imposible, muestra "Motor no disponible" con instrucciones.
   - Con las carpetas de datos y configuración del perfil sin permisos de lectura (salvo la de ejecución), la TUI funciona igual: no las abre.
   - **Par del canal** (L-06): con el directorio del socket en 0755 o de otro uid, o un servidor falso de otro uid, la TUI pasa a "Canal rechazado" sin enviar el handshake.
7. **V7 · Accesibilidad**: `NO_COLOR` → buffer sin colores; `--ascii` → solo ASCII; 79×24 → solo el mensaje de tamaño mínimo; `--plain` → sin secuencias de pantalla alternativa ni de cursor.
8. **V8 · i18n**: todo código del contrato (N7) tiene mensaje en y es (compila). Snapshots en/es.
9. **V9 · Editor**: un editor falso de terminal en un directorio temporal. La TUI suspende, no consume teclas mientras tanto y redibuja al volver. `vim; rm -rf ~` se rechaza, también si llega del daemon (revalidación, L-06). Un editor gráfico falso no bloquea. El editor recibe la ruta absoluta (I-03). Si `cockpit.editor` cambia entre dos aperturas, la segunda muestra el argv nuevo y pide confirmación (L-05). **Pendiente: etapa de validación multiplataforma** (Linux, Windows).
10. **V10 · CLI**: snapshot del esquema `--json` de `status` y `conflicts`, sin campos fuera de la allowlist. Mismo contenido que la TUI para la misma instantánea. Controles escapados.
11. **V11 · Varias TUIs**: dos `App` sin pantalla ven el mismo estado, y una acción anunciada aparece en ambas (BR-CKP-CONS-004).
12. **V12 · Capa y preferencias** (M-03, L-05): una TUI lanzada desde el árbol de un agente simulado muestra "actúas como claude-1", desactiva integrar, descartar y Cancelar con su motivo, y no guarda preferencias. Un filtro activo no oculta el recuento de ⚡ y ⛔ de la cabecera. Una escritura de preferencias con `cockpit.editor` se rechaza por esquema.

## Enmiendas que implica (no aplicadas)

**Estado (2026-10-05)**: E1 a E5 y E7 aplicadas como "Enmienda (2026-10-04, Cockpit)" en el documento de destino. E6: N1 a N7 aplicados en TS-GRP-004 (protocolo 6, 2026-10-05); N8 a N10 pendientes.

| # | Documento | Qué debe decir | Origen |
|---|---|---|---|
| E1 | **ADR-GRP-004** | Nueva sección aplicable al MVP, "Salida segura en CLI/TUI (SEC-12)": (a) todo texto que el contrato marca como no confiable pasa por un único saneador antes de mostrarse. Los C0, DEL, C1, bidi y de anchura cero se hacen visibles, nunca se emiten; recorte por anchura y longitud. (b) El saneado se impone por tipo: la presentación solo acepta texto saneado o del catálogo i18n. (c) La salida para máquinas (`--json`) escapa los mismos caracteres como `\uXXXX`. (d) El mecanismo del MVP es ADR-CKP-003 § 8. En la Fase 3, React nunca inserta texto no confiable como HTML y aplica las mismas categorías. Añadir también que, en el MVP, el modelo de estado de la TUI es el de ADR-CKP-003 (TEA) y que el § 1 y el § 2 siguen siendo de la Fase 3. Cierra M8 en ADR-GRP-004 y el punto 3 del § 10 del overview | DEP-CKP-9 |
| E2 | **ADR-GRP-011** § 3 y § 4 | Definir `t_client_recv` (fin de la lectura del mensaje, antes de deserializar) y `t_render` (vuelta de `draw` del primer frame tras aplicarlo). Añadir el gate "p95 del Cockpit > 100 ms: el CI falla", simétrico al del motor, y el aviso de feedback por tecla. El cliente sin pantalla del banco es la `App` de `apps/cli` sobre `TestBackend` | § 6, V3 |
| E3 | **ADR-GRP-009** Validación 5 | Módulos autorizados para lanzar procesos fuera de `crates/git`: `apps/cli` `tui::editor` (editor, argv fijo, sin shell) y la biblioteca cliente de `crates/api` (autoarranque). Cierra el punto 5 del § 10 del overview | DEP-CKP-12 |
| E4 | **ADR-GRP-007 / ADR-GRP-008** | La clave del editor (argv y clasificación terminal/gráfico) solo se admite en el perfil y en el local personal | DEP-CKP-13 |
| E5 | **ADR-GRP-006** | Preferencias de la TUI por usuario en el perfil, escritas solo por el daemon, documento acotado | DEP-CKP-11 |
| E6 | **TS-GRP-004 / api-contract-ipc.md** | N1 a N11 del § 4. N1 a N7 aplicados (2026-10-05, protocolo 6); N8 a N10 pendientes; N11 cubierto por TS-CKP-002 | DEP-CKP-6, 11, 13 |
| E7 | **DSYS-GRP-001** (no es ADR) | Símbolos como tokens con fallback ASCII y anchura. `crates/theme` agnóstico de `ratatui`. Alcance de `--plain` en el MVP. `ratatui` 0.30 en § 7 | § 7, § 10 |

## Revisión de seguridad (2026-10-04)

**Decisión del orquestador (2026-10-04), validada por Arquitecto, PO y security-expert.** Pasada de endurecimiento con los hallazgos que afectan a la TUI y a la CLI y el ajuste del PO sobre `--plain`. Los del ejecutor están en ADR-CKP-002 y los del predictor, en ADR-CKP-001.

| Hallazgo o ajuste | Dónde quedó resuelto |
|---|---|
| L-03 · Categorías de saneado | § 8 (U+061C, U+2028/2029, U+2060 a U+2064, Tags; tope de 100 caracteres en nombres; `--json`; mismas categorías en el MCP); SEC-12 de `non-functional.md`; V4 |
| L-05 · Preferencias, filtros y argv del editor | § 4 (N9), § 7 (cabecera), § 9 (argv visible y aprobado), § 10 (escritura solo de "sin atribuir" con controles 1 a 3; claves excluidas); V9 y V12 |
| L-06 · Par del canal y revalidación del argv | § 4 (comprobación antes del handshake; dónde vive: pendiente de TS-GRP-004), § 9 (revalidación); V6 y V9 |
| I-03 · Ruta absoluta al editor | § 4 (N10), § 9; V9 |
| M-03 · Capa fijada por el daemon (efecto en la TUI) | § 2 (acciones según la capa), § 4 (N5); V12 |
| PO · `--plain` | § 10 y Consecuencias: cita R-CKP-10 de CTX-CKP-001; el supuesto se retira |

## Referencias

- Requerimiento: [CTX-CKP-001](../../requirements/features/cockpit/context.md) (Q-CKP-1, 9, 16, 17, 18, 19, 20, 22, 23, 28; DEP-CKP-6, 9, 11, 12, 13) y [BR-CKP-001](../../requirements/features/cockpit/business-rules.md) (VAL-002, VAL-003, CALC-001, WF-004, CONS-001, CONS-004, CONS-006, CONS-007, TIME-001, EDGE-005, EDGE-006, EDGE-007).
- ADRs: ADR-GRP-001, 002, 003, 004, 005, 006, 009, 011, 013; ADR-TMC-004, 005; ADR-GRD-007; ADR-CKP-001 (predicción) y ADR-CKP-002 (catálogo y ejecutor), del mismo feature.
- NFR: NFR-03, NFR-04, NFR-05, NFR-06, NFR-09, NFR-10; SEC-01, SEC-08, SEC-10, SEC-12 ([non-functional.md](../non-functional.md)).
- Enablers: TS-GRP-004 (canal), INF-GRP-001 (repo intacto), INF-GRP-002 (banco de frescura).
- Design system: [DSYS-GRP-001](../../design-system/README.md) § 2, § 3, § 6, § 7.
- Versiones: crates.io (`ratatui` 0.30.2, `crossterm` 0.29.0), consultado el 2026-10-04; notas de la 0.30 en ratatui.rs.
