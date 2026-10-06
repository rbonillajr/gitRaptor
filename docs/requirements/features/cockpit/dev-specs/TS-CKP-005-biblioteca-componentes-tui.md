---
id: DS-TS-CKP-005
title: "Dev Spec — Biblioteca de componentes TUI v0"
type: dev-spec
status: approved
feature: cockpit
domain: GRP
story: TS-CKP-005
created: 2026-10-05
updated: 2026-10-05
related:
  adrs: [ADR-CKP-003, ADR-GRP-003, ADR-GRP-002]
  nfrs: [NFR-09, NFR-10]
  rules: [BR-CKP-EDGE-005, BR-CKP-EDGE-006]
tags: [cockpit, tui, ratatui, widgets, componentes, snapshots, insta, galeria, accesibilidad, safetext, sec-12]
---

# Dev Spec — TS-CKP-005: biblioteca de componentes TUI v0

Blueprint compacto de [TS-CKP-005](../technical-stories/TS-CKP-005-biblioteca-componentes-tui.md). Fuentes: DSYS-GRP-001 § 3 y § 6, ADR-CKP-003 § 7, § 8, § 10 y § 12, y TS-CKP-004 (tema). Solo widgets, snapshots y galería: sin bucle, sin canal y sin pantallas.

## 1. Ubicación en el código

| Archivo | Responsabilidad |
|---|---|
| `apps/cli/src/tui/style.rs` | `Styles` (tema → `ratatui::style`), `Glyphs` (estructurales, Unicode y ASCII), `Pen` (escritura en una línea), `panel`, `put`, `symbol`, `wrap`, `paragraph`, `follow`, `modal_area` |
| `apps/cli/src/tui/widgets/mod.rs` | Convención: trait `Component`, `Themed` y `themed()` |
| `apps/cli/src/tui/widgets/{layout,agent_list,graph_lanes,diff_view,timeline,conflict_alert,policy_banner,confirm,notification,key_hints}.rs` | Un componente por archivo, cada uno con su modelo de vista |
| `apps/cli/src/tui/widgets/tests.rs` + `snapshots/` | Snapshots y comprobaciones de accesibilidad y de fronteras |
| `apps/cli/src/tui/gallery/stories.rs` | Datos de ejemplo por componente y estado, compartidos por la galería y los snapshots |
| `apps/cli/src/tui/gallery/mod.rs` | `raptor ui gallery`: modos, navegación pura, bucle propio de desarrollo y `--dump` |
| `apps/cli/src/main.rs` | Subcomando oculto `ui gallery`, que llama a `gitraptor_cli::tui::gallery` |

Todo vive en la biblioteca interna de `apps/cli` creada por INF-CKP-001 (`gitraptor_cli`), junto a `tui::view`, que compone estos widgets.

## 2. Dependencias nuevas

| Dependencia | Dónde | Por qué | Licencia |
|---|---|---|---|
| `ratatui` 0.30 | ya en main (INF-CKP-001) | Los eventos de la galería usan `ratatui::crossterm` | MIT |
| `unicode-width` 0.2 | `apps/cli` | Anchura de visualización para recortar | MIT / Apache-2.0 |
| `insta` 1 | workspace, dev de `apps/cli` | Snapshots (DSYS-GRP-001 § 7) | Apache-2.0 |

## 3. Convención de widget (la que adopta INF-CKP-001)

- Cada componente es un **modelo de vista** propio: datos simples, sin tipos del canal, del motor ni de `present`. `tui::view` (INF-CKP-001) lo construye a partir del `Model`; `present::ingest` no lo conoce.
- El modelo implementa `Component { fn render(&self, area, buf, &Styles); fn height(&self, width) -> u16 }`. Se dibuja con `frame.render_widget(themed(&modelo, &styles), area)`. `Themed` implementa `ratatui::widgets::Widget`.
- **Widgets puros**: sin reloj, sin temporizador y sin estado propio. La selección, el foco y el desplazamiento (`offset`) van en el modelo. El desplazamiento se guarda y se mueve con `follow(prev, selected, height)` para que la vista no salte. Las horas, antigüedades y cuentas atrás llegan como texto.
- **Texto**: todo texto visible, etiquetas incluidas, es un campo `SafeText` del modelo. Los widgets no tienen literales de usuario, así que el idioma es el del catálogo del llamador. Los números (recuentos, líneas, ahead/behind) los formatea el widget.
- **Modales y toasts**: el llamador los coloca con `modal_area(area, ancho, modelo.height(ancho))` y `ToastStackModel::area`.

## 4. Componentes y estados

| Componente | Modelo | Estados en la galería y en los snapshots |
|---|---|---|
| Layout | `layout(area, BodyPlan) -> Option<Regions>`, `StatusBarModel`, `TooSmallModel` | 80×24, 100×30, 120×40, 79×24 (tamaño mínimo); StatusBar: en vivo, reconectando, motor no disponible, actuando como agente |
| AgentList / AgentRow | `AgentListModel`, `AgentRowModel` | estados mezclados (activo, inactivo, terminado, ⚡, ⛔, nombre recortado, HEAD separado, ahead/behind desconocido), noveno agente, operación en curso con Cancelar, no disponible, vacía, sin foco y desplazada |
| GraphLanes | `GraphModel`, `LaneModel` | tres agentes, carriles colapsados ("+N más"), commits sin atribuir, base no encontrada |
| DiffView | `DiffModel`, `DiffLine` | texto con hunk, añadidas y eliminadas; binario; sin cambios |
| TimelineList | `TimelineModel`, `TimelineEntry` | entradas con ⟲ y acciones; vacía; aviso de purga; Deshacer desactivado con motivo |
| ConflictAlert | `ConflictAlertModel` | nuevo, vigente con antigüedad, desactualizado y recalculando, calculando, pendiente de base, no calculable, más archivos que filas |
| PolicyBanner | `PolicyBannerModel` | bloqueada (regla, motivo y alternativa), pendiente de aprobación con cuenta atrás, caducada sin acciones |
| ConfirmPrompt | `ConfirmModel` (`Choice::No` por defecto) | No por defecto con lo que se pierde, Sí seleccionado, con recuperación ⟲ |
| Notification | `ToastStackModel`, `ToastModel` | éxito con "u deshacer", error, pila de tres |
| KeyHints / Help | `KeyHintsModel`, `HelpModel`, `KeyHint` | barra, barra estrecha (la ayuda `?` siempre queda), acción desactivada con motivo, superposición `?` |

**Layout (ADR-CKP-003 § 7)**: cabecera de una línea, cuerpo y KeyHints de una línea. Por debajo de 80×24 devuelve `None` y se pinta `TooSmallModel`, que conserva la tecla de salir. Por debajo de 120 columnas, la lista tiene al menos 10 filas, las alertas hasta un tercio del cuerpo y el grafo colapsa primero; el detalle se abre a pantalla completa bajo demanda. Desde 120 columnas, el grafo y el detalle van en una columna a la derecha.

## 5. Decisiones

Cada una es **decisión del orquestador (2026-10-05), validada por Arquitecto y PO** salvo que se diga otra cosa.

| # | Decisión | Por qué |
|---|---|---|
| D1 | Módulos de `apps/cli` (`tui::widgets`, `tui::style`, `tui::gallery`), sin crate nuevo | ADR-CKP-003 § 12 ya descartó `crates/tui`. `tui/mod.rs` solo declara módulos, para chocar lo mínimo con INF-CKP-001 (Arquitecto) |
| D2 | Los widgets solo aceptan el `SafeText` de INF-CKP-001 (`present::SafeText`, cuyos constructores sanean siempre). La galería construye sus datos de ejemplo con `SafeText::text` | Al integrar con INF-CKP-001 se retiró el tipo provisional de esta TS (solo tenía `trusted(&'static str)`): hay un solo tipo y ningún constructor envuelve texto sin sanear (§ 8) |
| D3 | Los glifos estructurales (bordes, carriles, elipsis, marcador de foco, flechas de ahead/behind, signos del diff) **no son tokens**: viven en `Glyphs`, con juego Unicode y ASCII elegido por el `SymbolSet` del tema | Son forma, no significado. Se registra como enmienda de DSYS-GRP-001 § 2.2. El nodo del grafo no reutiliza `●`, que es el token de agente activo (Arquitecto) |
| D4 | El foco se ve por **forma**: borde grueso (`┏━┓`, en ASCII `#=#`) y marcador `›` (en ASCII `>`) en el título y en la fila. `Styles` no añade atributos: `focus.default` ya resuelve negrita e inverso sin color | Forzar el inverso en los modos con color pisaría el tema. `›` evita la anchura ambigua de `▶` (Arquitecto) |
| D5 | La cabecera exige los recuentos de ⚡ y ⛔ como campos obligatorios y los pinta primero, desde la derecha | Ningún filtro ni texto largo puede ocultar una alerta (L-05, Arquitecto) |
| D6 | Snapshot por componente y estado: el texto se pinta una vez por juego de símbolos y el test exige que los cinco modos de color den el mismo texto. Solo cambian los tramos de estilo, que se listan por modo. El primer estado de cada componente y todos los de Layout cruzan además ASCII con cada modo de color | Cubre "cada estado en los 6 modos" (PO) y el cruce de juegos de símbolos de V1 (Arquitecto) sin multiplicar archivos: 45 snapshots |
| D7 | La galería es oculta (`hide = true`), con datos de ejemplo solo en inglés, sin canal ni escrituras, con sus propias teclas (no entran en `keymap`). Usa `ratatui::init()` y `restore()` (hook de pánico). `--dump` es la captura en texto | Es herramienta de desarrollo, no superficie de usuario: no sale en `--help` ni en la documentación de usuario (PO) |
| D8 | Las anchuras de columna de AgentList salen del tema activo: la del símbolo de estado y la de las marcas ⚡ ⛔ se miden en el juego en uso. Se descarta primero la actividad y después ahead/behind | ADR-CKP-003 § 7: el layout toma la anchura del tema, no del texto |
| D9 | La paleta es provisional y habrá una variante para terminal clara (rama `feat/TS-CKP-004-palette-a-light-terminals`). Los widgets solo piden tokens semánticos y la lista de modos de los snapshots sale de `Mode::ALL` de la galería: la variante clara entra como un valor más de `Mode`, sin tocar ningún widget. Los snapshots se regeneran con `INSTA_UPDATE=always cargo test -p gitraptor-cli --bin raptor tui::widgets` | Indicación del coordinador (2026-10-05). La API del tema para la variante clara se acuerda por PR con esa rama |

## 6. Plan de tests

| Test | Qué fija |
|---|---|
| `every_component_state_and_mode` | 45 snapshots con texto Unicode y ASCII y estilos en los modos; el texto no depende del modo de color |
| `states_differ_without_color` | Sin color, dos estados de un componente nunca coinciden (Unicode y ASCII) |
| `the_ascii_set_paints_only_ascii` | El fallback cae de verdad |
| `focus_has_a_shape_not_only_a_color`, `confirm_prompt_starts_on_no`, `key_hints_keep_help_whatever_the_width`, `the_header_always_shows_the_alert_counts`, `below_the_minimum_only_the_size_message_and_quit_are_painted`, `the_ninth_agent_reuses_the_first_color_and_keeps_its_name` | Accesibilidad de DSYS-GRP-001 § 6, BR-CKP-EDGE-005 y 006, y L-05 |
| `widgets_are_pure_and_literal_free` y `tests/tui_boundaries.rs` (INF-CKP-001) | Fronteras de ADR-CKP-003 V5: los widgets no usan el contrato ni el catálogo, ni colores, glifos o textos literales |
| `layout::tests`, `style::tests`, `gallery::tests` | Regiones por tamaño, `follow`, `modal_area`, recorte y ajuste de líneas, anchura de símbolos, navegación y `--dump` |

Los tests no usan motor, canal, repos ni perfil.

## 7. Fuera de alcance (y a quién pertenece)

- Bucle TEA, cliente del canal, `present::sanitize`, catálogo i18n tipado, `keymap` y target de biblioteca: **INF-CKP-001** (en main). Sustituir los marcadores de posición de `tui::view` por estos widgets: las **US-CKP** de cada vista.
- Pantallas y flujos (qué componente va en cada vista y con qué datos): **US-CKP**.
- Detección del modo (`--no-color`, `NO_COLOR`, `--theme high-contrast`, `COLORTERM`, `--ascii`, locale) y `--plain`: **US-CKP-005** e INF-CKP-001.
- Caducidad de 5 s del toast y su historial: update de **INF-CKP-001**.
- Acento y paleta finales (DSYS-GRP-001 § 8): siguen abiertos. Los snapshots se regeneran cuando se cierren.
- Linux y Windows (anchura de símbolos, consola de Windows, la galería en vivo): **Pendiente: etapa de validación multiplataforma**.
