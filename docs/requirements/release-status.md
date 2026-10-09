<!-- GENERADO: no editar a mano. Regenerar con `node tools/status/release-status.mjs`. -->

# Estado de release

> **Archivo generado, no editar a mano.** Sale del frontmatter (`status`) y de la sección "Estado de la implementación" de cada ficha (US, TS, INF, SPIKE y TD) de `docs/requirements/features/`. Para cambiar un estado, edita la ficha y regenera con `node tools/status/release-status.mjs`. El CI (docs-lint) falla si este archivo no coincide con lo que genera el script.

Última actualización de una ficha: 2026-10-08.

## Totales por estado

| Estado | Fichas |
|---|---|
| Implementado (`implemented`) | 36 |
| Hecho (`done`) | 3 |
| Implementado en parte (`partially-implemented`) | 16 |
| Listo (`ready`) | 3 |
| Borrador (`draft`) | 84 |
| Bloqueado (`blocked`) | 1 |
| **Total** | **143** |

## Por feature

| Feature | Implementado | Hecho | Implementado en parte | Listo | Borrador | Bloqueado | Total |
|---|---|---|---|---|---|---|---|
| [cockpit](#cockpit) | 6 | 0 | 1 | 0 | 25 | 1 | 33 |
| [guardrails](#guardrails) | 6 | 0 | 5 | 0 | 13 | 0 | 24 |
| [mcp](#mcp) | 5 | 0 | 0 | 0 | 15 | 0 | 20 |
| [motor-local](#motor-local) | 11 | 2 | 8 | 3 | 14 | 0 | 38 |
| [time-machine](#time-machine) | 8 | 1 | 2 | 0 | 17 | 0 | 28 |

### cockpit

33 fichas: 6 implementado, 1 implementado en parte, 25 borrador, 1 bloqueado.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [INF-CKP-001](features/cockpit/technical-stories/INF-CKP-001-esqueleto-tui.md) | Esqueleto de la TUI: cliente del canal, bucle TEA, saneado único y gate de 100 ms | Implementado (`implemented`) | #100, #139, #174, #153 | — |
| [TS-CKP-002](features/cockpit/technical-stories/TS-CKP-002-catalogo-ejecutor.md) | Catálogo de operaciones de usuario y ejecutor del daemon en dos fases | Implementado (`implemented`) | #73 | — |
| [TS-CKP-004](features/cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md) | Tokens semánticos y símbolos con fallback en el tema de la TUI | Implementado (`implemented`) | #50, #115 | — |
| [TS-CKP-005](features/cockpit/technical-stories/TS-CKP-005-biblioteca-componentes-tui.md) | Biblioteca de componentes TUI v0: los 10 widgets del design system, con snapshots y galería | Implementado (`implemented`) | #111, #115 | — |
| [US-CKP-001](features/cockpit/user-stories/US-CKP-001-flota-en-vivo.md) | El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo | Implementado (`implemented`) | #118, #126, #129, #130, #131, #136, #175 | — |
| [US-CKP-026](features/cockpit/user-stories/US-CKP-026-autoria-en-la-flota.md) | El desarrollador ve en la flota de quién es el último commit de cada worktree y con qué agente | Implementado (`implemented`) | #147 | — |
| [US-CKP-025](features/cockpit/user-stories/US-CKP-025-tui-repo-no-observado.md) | La TUI ofrece observar el repo en el que se abre y, fuera de un repo, lleva a los ya observados | Implementado en parte (`partially-implemented`) | #165, #170 | 2 |
| [SPIKE-CKP-001](features/cockpit/technical-stories/SPIKE-CKP-001-prediccion-5s.md) | Predicción de conflictos en ≤ 5 s p95 sin escribir en el repo: merge en memoria frente a almacén en el perfil | Borrador (`draft`) | — | — |
| [TS-CKP-001](features/cockpit/technical-stories/TS-CKP-001-predictor-conflictos.md) | Predictor de conflictos en el daemon: solape y conflicto previsto publicados para todos los clientes | Borrador (`draft`) | — | — |
| [TS-CKP-003](features/cockpit/technical-stories/TS-CKP-003-capa-cockpit-guardrails.md) | Capa cockpit en la decisión de Guardrails, ligada al git del ejecutor y registrada una sola vez | Borrador (`draft`) | — | — |
| [US-CKP-002](features/cockpit/user-stories/US-CKP-002-orden-atencion-terminadas.md) | La lista pone primero lo que pide atención y no se llena de sesiones viejas | Borrador (`draft`) | — | — |
| [US-CKP-003](features/cockpit/user-stories/US-CKP-003-estados-motor-conexion.md) | La TUI dice en qué estado está el motor y qué hacer en cada caso | Borrador (`draft`) | — | — |
| [US-CKP-004](features/cockpit/user-stories/US-CKP-004-selector-repo-preferencias.md) | El desarrollador cambia de repo y la TUI recuerda cómo la dejó | Borrador (`draft`) | — | — |
| [US-CKP-005](features/cockpit/user-stories/US-CKP-005-terminal-pequena-sin-color.md) | La TUI se puede usar en una terminal pequeña, sin color o en ASCII | Borrador (`draft`) | — | — |
| [US-CKP-006](features/cockpit/user-stories/US-CKP-006-conflictos-previstos.md) | El desarrollador ve qué agentes van a chocar antes de hacer merge | Borrador (`draft`) | — | — |
| [US-CKP-007](features/cockpit/user-stories/US-CKP-007-frescura-prediccion.md) | Una predicción vieja nunca se presenta como actual | Borrador (`draft`) | — | — |
| [US-CKP-008](features/cockpit/user-stories/US-CKP-008-alerta-conflicto-nuevo.md) | El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo | Borrador (`draft`) | — | — |
| [US-CKP-009](features/cockpit/user-stories/US-CKP-009-base-pendiente.md) | Con la rama base sin confirmar, la predicción contra la base queda pendiente | Borrador (`draft`) | — | — |
| [US-CKP-010](features/cockpit/user-stories/US-CKP-010-kpi-deteccion.md) | El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir | Borrador (`draft`) | — | — |
| [US-CKP-011](features/cockpit/user-stories/US-CKP-011-cli-solo-lectura.md) | El desarrollador consulta la flota y los conflictos desde la línea de comandos | Borrador (`draft`) | — | — |
| [US-CKP-012](features/cockpit/user-stories/US-CKP-012-ver-diff.md) | El desarrollador revisa lo que un agente integraría antes de hacer merge | Borrador (`draft`) | — | — |
| [US-CKP-013](features/cockpit/user-stories/US-CKP-013-abrir-en-editor.md) | El desarrollador abre el worktree de un agente en su editor | Borrador (`draft`) | — | — |
| [US-CKP-014](features/cockpit/user-stories/US-CKP-014-integrar-rama-agente.md) | El desarrollador integra la rama de un agente con una tecla y puede deshacerlo | Borrador (`draft`) | — | — |
| [US-CKP-015](features/cockpit/user-stories/US-CKP-015-rebasar-rama-agente.md) | El desarrollador pone al día la rama de un agente sobre la base | Borrador (`draft`) | — | — |
| [US-CKP-016](features/cockpit/user-stories/US-CKP-016-operacion-detenida.md) | Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir | Borrador (`draft`) | — | — |
| [US-CKP-017](features/cockpit/user-stories/US-CKP-017-descartar-worktree.md) | El desarrollador descarta el trabajo de un agente sin miedo a perderlo | Borrador (`draft`) | — | — |
| [US-CKP-018](features/cockpit/user-stories/US-CKP-018-crear-worktree.md) | El desarrollador prepara un worktree para un agente nuevo | Borrador (`draft`) | — | — |
| [US-CKP-019](features/cockpit/user-stories/US-CKP-019-denegada-excepcion-consciente.md) | El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente | Borrador (`draft`) | — | — |
| [US-CKP-020](features/cockpit/user-stories/US-CKP-020-trabajo-de-otro-actor.md) | Tocar el trabajo de otro actor exige confirmar el plan concreto | Borrador (`draft`) | — | — |
| [US-CKP-021](features/cockpit/user-stories/US-CKP-021-historial-aviso-purga.md) | El desarrollador ve en la TUI el historial de operaciones y el aviso de purga | Borrador (`draft`) | — | — |
| [US-CKP-022](features/cockpit/user-stories/US-CKP-022-grafo-carriles.md) | El desarrollador ve crecer la rama de cada agente sobre la base | Borrador (`draft`) | — | — |
| [US-CKP-024](features/cockpit/user-stories/US-CKP-024-integrar-casos-limite.md) | Integrar sigue siendo seguro cuando el estado cambia, choca o la base no está sacada | Borrador (`draft`) | — | — |
| [US-CKP-023](features/cockpit/user-stories/US-CKP-023-cola-confirmacion.md) | El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera | Bloqueado (`blocked`) | — | — |

### guardrails

24 fichas: 6 implementado, 5 implementado en parte, 13 borrador.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [INF-GRD-001](features/guardrails/technical-stories/INF-GRD-001-arnes-hooks.md) | Arnés de la capa de hooks: repos con hooks previos, huella, interrupción y matriz de CI | Implementado (`implemented`) | #65, #121, #116 | — |
| [TS-GRD-001](features/guardrails/technical-stories/TS-GRD-001-configuracion-commiteada.md) | Lectura commiteada de la configuración del equipo y de la rama principal | Implementado (`implemented`) | #27 | — |
| [US-GRD-001](features/guardrails/user-stories/US-GRD-001-proteger-repo-force-push-bloqueado.md) | Un agente que intenta hacer force-push en un repo protegido queda bloqueado | Implementado (`implemented`) | #116, #121 | — |
| [US-GRD-002](features/guardrails/user-stories/US-GRD-002-hooks-previos-respetados.md) | Los hooks que el repo ya tenía siguen funcionando al protegerlo | Implementado (`implemented`) | #193 | — |
| [US-GRD-008](features/guardrails/user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md) | Ningún agente cambia una rama protegida ni toca una ruta prohibida | Implementado (`implemented`) | #197 | — |
| [US-GRD-019](features/guardrails/user-stories/US-GRD-019-quien-ejecuto-y-a-nombre-de-quien.md) | El desarrollador ve quién ejecutó cada commit y a nombre de quién entró cuando no coinciden | Implementado (`implemented`) | #144, #147, #151, #175 | — |
| [SPIKE-GRD-001](features/guardrails/technical-stories/SPIKE-GRD-001-interceptabilidad-hooks.md) | Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO | Implementado en parte (`partially-implemented`) | #22, #25, #121 | 2 |
| [US-GRD-003](features/guardrails/user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md) | El desarrollador retira la protección y el repo queda exactamente como estaba | Implementado en parte (`partially-implemented`) | #193, #201 | — |
| [US-GRD-004](features/guardrails/user-stories/US-GRD-004-aviso-proteccion-inactiva.md) | El desarrollador se entera de que la protección de un repo dejó de estar activa | Implementado en parte (`partially-implemented`) | #201 | — |
| [US-GRD-005](features/guardrails/user-stories/US-GRD-005-registro-de-bloqueos.md) | El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo | Implementado en parte (`partially-implemented`) | #154, #168 | 3 |
| [US-GRD-018](features/guardrails/user-stories/US-GRD-018-autoria-commits-persona-y-agente.md) | Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo | Implementado en parte (`partially-implemented`) | #137, #141, #150, #152, #153, #154 | 3 |
| [SPIKE-GRD-002](features/guardrails/technical-stories/SPIKE-GRD-002-factor-so-daemon.md) | Factor de autenticación del SO invocado desde el daemon en los tres SO | Borrador (`draft`) | — | — |
| [TD-GRD-001](features/guardrails/technical-stories/TD-GRD-001-dispatcher-plantilla-3-pre-push-toda-ref.md) | Dispatcher plantilla 3: evaluar el pre-push de toda ref empujada (tags, notes y demás refs no gobernadas) | Borrador (`draft`) | — | — |
| [US-GRD-006](features/guardrails/user-stories/US-GRD-006-excepcion-consciente.md) | El desarrollador hace a conciencia una operación prohibida y queda constancia | Borrador (`draft`) | — | — |
| [US-GRD-007](features/guardrails/user-stories/US-GRD-007-permisos-por-operacion.md) | El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación | Borrador (`draft`) | — | — |
| [US-GRD-009](features/guardrails/user-stories/US-GRD-009-tamano-diff-formato-commit.md) | Los agentes entregan commits pequeños y con el formato que exige el equipo | Borrador (`draft`) | — | — |
| [US-GRD-010](features/guardrails/user-stories/US-GRD-010-endurecer-sin-relajar.md) | El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo | Borrador (`draft`) | — | — |
| [US-GRD-011](features/guardrails/user-stories/US-GRD-011-configuracion-ilegible.md) | Una configuración rota no deja pasar las operaciones peligrosas | Borrador (`draft`) | — | — |
| [US-GRD-012](features/guardrails/user-stories/US-GRD-012-agente-no-relaja-configuracion.md) | Un agente no puede relajar las reglas del equipo cambiando su configuración | Borrador (`draft`) | — | — |
| [US-GRD-013](features/guardrails/user-stories/US-GRD-013-comando-edicion-configuracion.md) | El desarrollador cambia la configuración con un comando sin perder lo que editó a mano | Borrador (`draft`) | — | — |
| [US-GRD-014](features/guardrails/user-stories/US-GRD-014-rama-base-del-equipo.md) | El equipo fija la rama base del repo y Guardrails la protege | Borrador (`draft`) | — | — |
| [US-GRD-015](features/guardrails/user-stories/US-GRD-015-cola-de-confirmacion.md) | El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera | Borrador (`draft`) | — | — |
| [US-GRD-016](features/guardrails/user-stories/US-GRD-016-misma-decision-por-mcp.md) | Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo | Borrador (`draft`) | — | — |
| [US-GRD-017](features/guardrails/user-stories/US-GRD-017-sin-snapshot-no-se-ejecuta.md) | Una operación destructiva permitida no se ejecuta sin un punto de recuperación | Borrador (`draft`) | — | — |

### mcp

20 fichas: 5 implementado, 15 borrador.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [US-MCP-001](features/mcp/user-stories/US-MCP-001-instalar-en-claude-code.md) | El desarrollador conecta Claude Code a GitRaptor con un solo comando y lo retira igual de fácil | Implementado (`implemented`) | #70 | — |
| [US-MCP-002](features/mcp/user-stories/US-MCP-002-habilitar-repo-allowlist.md) | El desarrollador decide qué repos pueden usar los agentes por el MCP | Implementado (`implemented`) | #140, #159 | — |
| [US-MCP-003](features/mcp/user-stories/US-MCP-003-status-del-repo-del-agente.md) | Un agente consulta por MCP el estado del repo en el que trabaja, y de ningún otro | Implementado (`implemented`) | #140, #159, #153 | — |
| [US-MCP-005](features/mcp/user-stories/US-MCP-005-respuestas-acotadas-y-seguras.md) | Un agente recibe respuestas acotadas que no pueden darle órdenes ni filtrar secretos | Implementado (`implemented`) | #157, #159 | — |
| [US-MCP-008](features/mcp/user-stories/US-MCP-008-snapshot-manual.md) | Un agente guarda por MCP un punto de recuperación antes de un cambio arriesgado | Implementado (`implemented`) | #213 | — |
| [INF-MCP-001](features/mcp/technical-stories/INF-MCP-001-corpus-seguridad-mcp.md) | Corpus de seguridad del MCP como suite de CI que bloquea el merge | Borrador (`draft`) | — | — |
| [US-MCP-004](features/mcp/user-stories/US-MCP-004-status-completo-y-no-disponible.md) | Un agente sabe por MCP quién más trabaja en su repo, contra qué base y con qué protección | Borrador (`draft`) | — | — |
| [US-MCP-006](features/mcp/user-stories/US-MCP-006-registrar-agente.md) | Un agente sin soporte completo se declara por MCP y su trabajo queda a su nombre | Borrador (`draft`) | — | — |
| [US-MCP-007](features/mcp/user-stories/US-MCP-007-confused-deputy.md) | Un hook lanzado por una escritura de un agente no puede usar los poderes del desarrollador | Borrador (`draft`) | — | — |
| [US-MCP-009](features/mcp/user-stories/US-MCP-009-safe-commit.md) | Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo | Borrador (`draft`) | — | — |
| [US-MCP-010](features/mcp/user-stories/US-MCP-010-safe-commit-rutas.md) | Un agente commitea solo los archivos que nombra y nunca fuera de su worktree | Borrador (`draft`) | — | — |
| [US-MCP-011](features/mcp/user-stories/US-MCP-011-escritura-en-el-worktree-correcto.md) | La escritura de un agente cae en el worktree que espera, o no ocurre | Borrador (`draft`) | — | — |
| [US-MCP-012](features/mcp/user-stories/US-MCP-012-undo-de-lo-propio.md) | Un agente deshace por MCP su última operación sin tocar el trabajo de otros | Borrador (`draft`) | — | — |
| [US-MCP-013](features/mcp/user-stories/US-MCP-013-confirmacion-sin-cola.md) | Un agente sabe que una acción necesita al desarrollador y a quién acudir | Borrador (`draft`) | — | — |
| [US-MCP-014](features/mcp/user-stories/US-MCP-014-confirmacion-pendiente-en-cola.md) | La acción de riesgo de un agente queda en espera del desarrollador sin bloquear al agente | Borrador (`draft`) | — | — |
| [US-MCP-015](features/mcp/user-stories/US-MCP-015-medicion-del-mcp.md) | El desarrollador mide cuánto paró y cuánto recuperó el MCP | Borrador (`draft`) | — | — |
| [US-MCP-016](features/mcp/user-stories/US-MCP-016-check-conflicts.md) | Un agente sabe con quién va a chocar antes de que choque | Borrador (`draft`) | — | — |
| [US-MCP-017](features/mcp/user-stories/US-MCP-017-explain-history.md) | Un agente entiende qué pasó en su repo sin ver mensajes ni contenido | Borrador (`draft`) | — | — |
| [US-MCP-018](features/mcp/user-stories/US-MCP-018-safe-rebase-atomico.md) | Un agente se pone al día con la rama base y, si choca, su rama queda como estaba | Borrador (`draft`) | — | — |
| [US-MCP-019](features/mcp/user-stories/US-MCP-019-create-worktree.md) | Un agente prepara un worktree nuevo desde la rama base para otro trabajo | Borrador (`draft`) | — | — |

### motor-local

38 fichas: 11 implementado, 2 hecho, 8 implementado en parte, 3 listo, 14 borrador.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [TS-GRP-003](features/motor-local/technical-stories/TS-GRP-003-proceso-motor.md) | Proceso del motor en segundo plano por usuario | Implementado (`implemented`) | #26 | — |
| [TS-GRP-006](features/motor-local/technical-stories/TS-GRP-006-observacion-por-niveles.md) | Observación por niveles: activo, dormido con centinela y despertar | Implementado (`implemented`) | #164, #167, #173, #178 | — |
| [US-GRP-001](features/motor-local/user-stories/US-GRP-001-estado-worktrees-repo-anadido.md) | El desarrollador ve el estado de cada worktree del repo que añadió | Implementado (`implemented`) | #46 | — |
| [US-GRP-002](features/motor-local/user-stories/US-GRP-002-cambios-eventos-en-vivo.md) | El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante | Implementado (`implemented`) | #57, #75 | — |
| [US-GRP-004](features/motor-local/user-stories/US-GRP-004-observacion-continua.md) | El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto | Implementado (`implemented`) | #112 | — |
| [US-GRP-007](features/motor-local/user-stories/US-GRP-007-sesiones-claude-code.md) | El desarrollador sabe qué sesión de Claude Code trabaja en cada worktree y si sigue activa | Implementado (`implemented`) | #80, #119, #124, #155, #169 | — |
| [US-GRP-009](features/motor-local/user-stories/US-GRP-009-registro-explicito-agente.md) | El desarrollador o el agente declaran qué agente trabaja en un worktree | Implementado (`implemented`) | #106 | — |
| [US-GRP-012](features/motor-local/user-stories/US-GRP-012-rama-base-main.md) | El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo | Implementado (`implemented`) | #55 | — |
| [US-GRP-017](features/motor-local/user-stories/US-GRP-017-consumo-recursos-status.md) | El desarrollador ve cuánto consume GitRaptor en su máquina | Implementado (`implemented`) | #93, #173 | — |
| [US-GRP-020](features/motor-local/user-stories/US-GRP-020-carpetas-codigo-repos-descubiertos.md) | El desarrollador ve los repos que aparecen en sus carpetas de código sin tener que añadirlos uno a uno | Implementado (`implemented`) | #170, #153, #178 | — |
| [US-GRP-022](features/motor-local/user-stories/US-GRP-022-aceptar-descartar-repo-descubierto.md) | El desarrollador decide qué repos descubiertos se observan y los que descarta no vuelven a aparecer | Implementado (`implemented`) | #170 | — |
| [TS-GRP-001](features/motor-local/technical-stories/TS-GRP-001-almacen-perfil.md) | Almacén de datos del motor en el perfil | Hecho (`done`) | — | — |
| [TS-GRP-002](features/motor-local/technical-stories/TS-GRP-002-lectura-git.md) | Capa de lectura de Git sin escrituras | Hecho (`done`) | — | — |
| [INF-GRP-001](features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md) | Arnés de verificación "repo intacto" en los tres SO | Implementado en parte (`partially-implemented`) | #30, #34, #68, #97, #109, #113, #163 | 3 |
| [INF-GRP-002](features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md) | Banco de medición de frescura y escala | Implementado en parte (`partially-implemented`) | #76, #98, #133 | 3 |
| [INF-GRP-003](features/motor-local/technical-stories/INF-GRP-003-pipeline-release.md) | Pipeline de release: 6 targets, firma, checksums, SBOM y borrador de GitHub Release | Implementado en parte (`partially-implemented`) | #51 | 3 |
| [INF-GRP-004](features/motor-local/technical-stories/INF-GRP-004-canales-distribucion.md) | Canales de distribución: Homebrew, winget, npm y scripts de instalación verificados | Implementado en parte (`partially-implemented`) | #51 | 2 |
| [SPIKE-GRP-001](features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md) | Precisión de la detección de Claude Code en dogfooding | Implementado en parte (`partially-implemented`) | #80, #155, #169 | 3 |
| [SPIKE-GRP-002](features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md) | Viabilidad del observador de cambios a escala en los tres SO | Implementado en parte (`partially-implemented`) | #17, #23 | 2 |
| [TD-GRP-001](features/motor-local/technical-stories/TD-GRP-001-acl-windows.md) | Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06) | Implementado en parte (`partially-implemented`) | #72 | 2 |
| [TS-GRP-004](features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md) | Canal local de clientes y contrato de mensajes | Implementado en parte (`partially-implemented`) | #36, #87, #123, #133, #158 | 3 |
| [TD-GRP-002](features/motor-local/technical-stories/TD-GRP-002-motor-bajo-rafaga.md) | Frescura y memoria del motor bajo una ráfaga de archivos | Listo (`ready`) | — | — |
| [TD-GRP-003](features/motor-local/technical-stories/TD-GRP-003-nfr04-maquina-referencia.md) | Presupuesto de NFR-04 sin gate automático: medirlo en una máquina de referencia | Listo (`ready`) | — | — |
| [TS-GRP-005](features/motor-local/technical-stories/TS-GRP-005-clases-trabajo-ahorro-energia.md) | Clases de trabajo del daemon y mecanismo de ahorro de energía | Listo (`ready`) | — | — |
| [TS-GRP-007](features/motor-local/technical-stories/TS-GRP-007-factor-so-reservados-windows.md) | Factor del SO (Windows Hello) para los comandos reservados de alto riesgo en Windows | Borrador (`draft`) | — | — |
| [US-GRP-003](features/motor-local/user-stories/US-GRP-003-estados-especiales-no-disponible.md) | El desarrollador sabe qué worktree está en un estado especial o ya no existe | Borrador (`draft`) | — | — |
| [US-GRP-005](features/motor-local/user-stories/US-GRP-005-hueco-sin-atribuir.md) | El desarrollador distingue lo que el motor no vio ocurrir | Borrador (`draft`) | — | — |
| [US-GRP-006](features/motor-local/user-stories/US-GRP-006-retirar-y-volver-a-anadir.md) | El desarrollador recupera el historial de un repo que retiró y volvió a añadir | Borrador (`draft`) | — | — |
| [US-GRP-008](features/motor-local/user-stories/US-GRP-008-editor-humano-sin-atribuir.md) | El trabajo que el desarrollador hace en su editor nunca se atribuye a Claude Code | Borrador (`draft`) | — | — |
| [US-GRP-010](features/motor-local/user-stories/US-GRP-010-corregir-atribucion.md) | El desarrollador corrige una atribución automática equivocada | Borrador (`draft`) | — | — |
| [US-GRP-011](features/motor-local/user-stories/US-GRP-011-worktree-compartido.md) | El desarrollador ve todas las sesiones de un worktree compartido | Borrador (`draft`) | — | — |
| [US-GRP-013](features/motor-local/user-stories/US-GRP-013-umbral-inactividad-por-repo.md) | El desarrollador ajusta para un repo cuándo una sesión pasa a inactiva | Borrador (`draft`) | — | — |
| [US-GRP-014](features/motor-local/user-stories/US-GRP-014-git-ausente-o-antiguo.md) | El desarrollador sabe qué Git le falta y el motor empieza solo cuando lo instala | Borrador (`draft`) | — | — |
| [US-GRP-015](features/motor-local/user-stories/US-GRP-015-primer-repo-maquina-nueva.md) | El desarrollador recién instalado sabe cómo añadir su primer repo | Borrador (`draft`) | — | — |
| [US-GRP-016](features/motor-local/user-stories/US-GRP-016-rama-base-configuracion-equipo.md) | La rama base la define la configuración del equipo | Borrador (`draft`) | — | — |
| [US-GRP-018](features/motor-local/user-stories/US-GRP-018-doctor-recursos.md) | El desarrollador diagnostica con raptor doctor si GitRaptor gasta de más | Borrador (`draft`) | — | — |
| [US-GRP-019](features/motor-local/user-stories/US-GRP-019-modo-ahorro-energia.md) | GitRaptor gasta menos batería cuando el portátil no está enchufado | Borrador (`draft`) | — | — |
| [US-GRP-021](features/motor-local/user-stories/US-GRP-021-raptor-clone.md) | El desarrollador clona un repo con raptor clone y decide en el momento si GitRaptor lo observa | Borrador (`draft`) | — | — |

### time-machine

28 fichas: 8 implementado, 1 hecho, 2 implementado en parte, 17 borrador.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [TS-TMC-001](features/time-machine/technical-stories/TS-TMC-001-almacen-captura-snapshots.md) | Almacén de snapshots en el perfil y captura de estado | Implementado (`implemented`) | #38, #127, #171 | — |
| [TS-TMC-002](features/time-machine/technical-stories/TS-TMC-002-oplog-diario.md) | Oplog de la Time Machine con diario de intención y recuperación | Implementado (`implemented`) | #32, #176 | — |
| [TS-TMC-003](features/time-machine/technical-stories/TS-TMC-003-escritura-aplicador.md) | Capa de escritura acotada y aplicador de estados | Implementado (`implemented`) | #45, #171, #172 | — |
| [TS-TMC-004](features/time-machine/technical-stories/TS-TMC-004-operacion-protegida-solicitante.md) | Operación protegida y resolución del solicitante en el canal | Implementado (`implemented`) | #40 | — |
| [US-TMC-001](features/time-machine/user-stories/US-TMC-001-snapshot-previo-operaciones-gitraptor.md) | El desarrollador recupera su trabajo sin commitear tras cualquier operación lanzada por GitRaptor | Implementado (`implemented`) | #61 | — |
| [US-TMC-002](features/time-machine/user-stories/US-TMC-002-undo-ultima-operacion.md) | El desarrollador deshace con un comando la última operación de su worktree | Implementado (`implemented`) | #90, #127, #146, #156 | — |
| [US-TMC-004](features/time-machine/user-stories/US-TMC-004-captura-continua-git-crudo.md) | El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable | Implementado (`implemented`) | #120, #127, #146 | — |
| [US-TMC-006](features/time-machine/user-stories/US-TMC-006-timeline-que-cuando-quien.md) | El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo | Implementado (`implemented`) | #198 | — |
| [SPIKE-TMC-001](features/time-machine/technical-stories/SPIKE-TMC-001-repo-mediano-overhead.md) | Repo mediano de referencia y viabilidad del snapshot en menos de 200 ms | Hecho (`done`) | — | — |
| [INF-TMC-001](features/time-machine/technical-stories/INF-TMC-001-arnes-caos-recuperable.md) | Arnés de caos y de garantías de los snapshots en los tres SO | Implementado en parte (`partially-implemented`) | — | — |
| [US-TMC-009](features/time-machine/user-stories/US-TMC-009-restaurar-punto-timeline.md) | El desarrollador devuelve su worktree a cualquier punto del timeline | Implementado en parte (`partially-implemented`) | #212 | — |
| [US-TMC-003](features/time-machine/user-stories/US-TMC-003-redo.md) | El desarrollador rehace lo que deshizo por error | Borrador (`draft`) | — | — |
| [US-TMC-005](features/time-machine/user-stories/US-TMC-005-snapshot-previo-hooks-guardrails.md) | Las operaciones de Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails | Borrador (`draft`) | — | — |
| [US-TMC-007](features/time-machine/user-stories/US-TMC-007-timeline-filtros-huecos.md) | El desarrollador filtra el timeline por worktree, agente o periodo y ve lo que no se observó | Borrador (`draft`) | — | — |
| [US-TMC-008](features/time-machine/user-stories/US-TMC-008-timeline-atribucion-vigente.md) | El timeline refleja las correcciones de atribución sin reescribir quién deshizo qué | Borrador (`draft`) | — | — |
| [US-TMC-010](features/time-machine/user-stories/US-TMC-010-undo-since.md) | El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos | Borrador (`draft`) | — | — |
| [US-TMC-011](features/time-machine/user-stories/US-TMC-011-undo-por-agente.md) | El desarrollador deshace solo lo que hizo un agente en un periodo | Borrador (`draft`) | — | — |
| [US-TMC-012](features/time-machine/user-stories/US-TMC-012-solape-otro-actor.md) | Un undo nunca sobrescribe trabajo posterior de otro actor | Borrador (`draft`) | — | — |
| [US-TMC-013](features/time-machine/user-stories/US-TMC-013-permisos-solicitante.md) | Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell | Borrador (`draft`) | — | — |
| [US-TMC-014](features/time-machine/user-stories/US-TMC-014-undo-ya-empujado.md) | El desarrollador sabe cuándo lo que deshizo sigue en el remoto | Borrador (`draft`) | — | — |
| [US-TMC-015](features/time-machine/user-stories/US-TMC-015-operacion-git-en-curso.md) | El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar | Borrador (`draft`) | — | — |
| [US-TMC-016](features/time-machine/user-stories/US-TMC-016-retencion-por-defecto.md) | Los snapshots no llenan el disco y nunca se pierde el último punto antes de una operación destructiva | Borrador (`draft`) | — | — |
| [US-TMC-017](features/time-machine/user-stories/US-TMC-017-retencion-configurable.md) | El desarrollador ajusta para sí cuánto tiempo se conservan los snapshots | Borrador (`draft`) | — | — |
| [US-TMC-018](features/time-machine/user-stories/US-TMC-018-garantias-snapshots.md) | Los snapshots no se publican, no se pierden con el mantenimiento de Git y ningún agente los altera | Borrador (`draft`) | — | — |
| [US-TMC-019](features/time-machine/user-stories/US-TMC-019-robustez-interrupcion.md) | El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo | Borrador (`draft`) | — | — |
| [US-TMC-020](features/time-machine/user-stories/US-TMC-020-overhead-snapshot.md) | El desarrollador y sus agentes no notan el coste de los snapshots | Borrador (`draft`) | — | — |
| [US-TMC-021](features/time-machine/user-stories/US-TMC-021-politica-guardrails-undo.md) | Las políticas del repo pueden restringir quién deshace, nunca ampliarlo | Borrador (`draft`) | — | — |
| [US-TMC-022](features/time-machine/user-stories/US-TMC-022-tope-disco.md) | La Time Machine nunca pasa del tope de disco que fijé | Borrador (`draft`) | — | — |

## Por hito

Hitos según [`release-plan.md`](release-plan.md) y el campo `milestone` de las fichas.

### M1

19 fichas: 18 implementado, 1 implementado en parte.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [INF-CKP-001](features/cockpit/technical-stories/INF-CKP-001-esqueleto-tui.md) | Esqueleto de la TUI: cliente del canal, bucle TEA, saneado único y gate de 100 ms | Implementado (`implemented`) | #100, #139, #174, #153 | — |
| [INF-GRD-001](features/guardrails/technical-stories/INF-GRD-001-arnes-hooks.md) | Arnés de la capa de hooks: repos con hooks previos, huella, interrupción y matriz de CI | Implementado (`implemented`) | #65, #121, #116 | — |
| [TS-TMC-004](features/time-machine/technical-stories/TS-TMC-004-operacion-protegida-solicitante.md) | Operación protegida y resolución del solicitante en el canal | Implementado (`implemented`) | #40 | — |
| [US-CKP-001](features/cockpit/user-stories/US-CKP-001-flota-en-vivo.md) | El desarrollador ve en vivo qué agente trabaja en cada worktree de un repo | Implementado (`implemented`) | #118, #126, #129, #130, #131, #136, #175 | — |
| [US-GRD-001](features/guardrails/user-stories/US-GRD-001-proteger-repo-force-push-bloqueado.md) | Un agente que intenta hacer force-push en un repo protegido queda bloqueado | Implementado (`implemented`) | #116, #121 | — |
| [US-GRP-001](features/motor-local/user-stories/US-GRP-001-estado-worktrees-repo-anadido.md) | El desarrollador ve el estado de cada worktree del repo que añadió | Implementado (`implemented`) | #46 | — |
| [US-GRP-002](features/motor-local/user-stories/US-GRP-002-cambios-eventos-en-vivo.md) | El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante | Implementado (`implemented`) | #57, #75 | — |
| [US-GRP-004](features/motor-local/user-stories/US-GRP-004-observacion-continua.md) | El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto | Implementado (`implemented`) | #112 | — |
| [US-GRP-007](features/motor-local/user-stories/US-GRP-007-sesiones-claude-code.md) | El desarrollador sabe qué sesión de Claude Code trabaja en cada worktree y si sigue activa | Implementado (`implemented`) | #80, #119, #124, #155, #169 | — |
| [US-GRP-009](features/motor-local/user-stories/US-GRP-009-registro-explicito-agente.md) | El desarrollador o el agente declaran qué agente trabaja en un worktree | Implementado (`implemented`) | #106 | — |
| [US-GRP-012](features/motor-local/user-stories/US-GRP-012-rama-base-main.md) | El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo | Implementado (`implemented`) | #55 | — |
| [US-GRP-017](features/motor-local/user-stories/US-GRP-017-consumo-recursos-status.md) | El desarrollador ve cuánto consume GitRaptor en su máquina | Implementado (`implemented`) | #93, #173 | — |
| [US-MCP-001](features/mcp/user-stories/US-MCP-001-instalar-en-claude-code.md) | El desarrollador conecta Claude Code a GitRaptor con un solo comando y lo retira igual de fácil | Implementado (`implemented`) | #70 | — |
| [US-MCP-002](features/mcp/user-stories/US-MCP-002-habilitar-repo-allowlist.md) | El desarrollador decide qué repos pueden usar los agentes por el MCP | Implementado (`implemented`) | #140, #159 | — |
| [US-MCP-003](features/mcp/user-stories/US-MCP-003-status-del-repo-del-agente.md) | Un agente consulta por MCP el estado del repo en el que trabaja, y de ningún otro | Implementado (`implemented`) | #140, #159, #153 | — |
| [US-TMC-001](features/time-machine/user-stories/US-TMC-001-snapshot-previo-operaciones-gitraptor.md) | El desarrollador recupera su trabajo sin commitear tras cualquier operación lanzada por GitRaptor | Implementado (`implemented`) | #61 | — |
| [US-TMC-002](features/time-machine/user-stories/US-TMC-002-undo-ultima-operacion.md) | El desarrollador deshace con un comando la última operación de su worktree | Implementado (`implemented`) | #90, #127, #146, #156 | — |
| [US-TMC-004](features/time-machine/user-stories/US-TMC-004-captura-continua-git-crudo.md) | El trabajo hecho fuera de GitRaptor queda capturado como punto recuperable | Implementado (`implemented`) | #120, #127, #146 | — |
| [SPIKE-GRP-001](features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md) | Precisión de la detección de Claude Code en dogfooding | Implementado en parte (`partially-implemented`) | #80, #155, #169 | 3 |

### M2

110 fichas: 18 implementado, 3 hecho, 10 implementado en parte, 3 listo, 75 borrador, 1 bloqueado.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [TS-CKP-002](features/cockpit/technical-stories/TS-CKP-002-catalogo-ejecutor.md) | Catálogo de operaciones de usuario y ejecutor del daemon en dos fases | Implementado (`implemented`) | #73 | — |
| [TS-CKP-004](features/cockpit/technical-stories/TS-CKP-004-tokens-semanticos-simbolos.md) | Tokens semánticos y símbolos con fallback en el tema de la TUI | Implementado (`implemented`) | #50, #115 | — |
| [TS-CKP-005](features/cockpit/technical-stories/TS-CKP-005-biblioteca-componentes-tui.md) | Biblioteca de componentes TUI v0: los 10 widgets del design system, con snapshots y galería | Implementado (`implemented`) | #111, #115 | — |
| [TS-GRD-001](features/guardrails/technical-stories/TS-GRD-001-configuracion-commiteada.md) | Lectura commiteada de la configuración del equipo y de la rama principal | Implementado (`implemented`) | #27 | — |
| [TS-GRP-003](features/motor-local/technical-stories/TS-GRP-003-proceso-motor.md) | Proceso del motor en segundo plano por usuario | Implementado (`implemented`) | #26 | — |
| [TS-GRP-006](features/motor-local/technical-stories/TS-GRP-006-observacion-por-niveles.md) | Observación por niveles: activo, dormido con centinela y despertar | Implementado (`implemented`) | #164, #167, #173, #178 | — |
| [TS-TMC-001](features/time-machine/technical-stories/TS-TMC-001-almacen-captura-snapshots.md) | Almacén de snapshots en el perfil y captura de estado | Implementado (`implemented`) | #38, #127, #171 | — |
| [TS-TMC-002](features/time-machine/technical-stories/TS-TMC-002-oplog-diario.md) | Oplog de la Time Machine con diario de intención y recuperación | Implementado (`implemented`) | #32, #176 | — |
| [TS-TMC-003](features/time-machine/technical-stories/TS-TMC-003-escritura-aplicador.md) | Capa de escritura acotada y aplicador de estados | Implementado (`implemented`) | #45, #171, #172 | — |
| [US-CKP-026](features/cockpit/user-stories/US-CKP-026-autoria-en-la-flota.md) | El desarrollador ve en la flota de quién es el último commit de cada worktree y con qué agente | Implementado (`implemented`) | #147 | — |
| [US-GRD-002](features/guardrails/user-stories/US-GRD-002-hooks-previos-respetados.md) | Los hooks que el repo ya tenía siguen funcionando al protegerlo | Implementado (`implemented`) | #193 | — |
| [US-GRD-008](features/guardrails/user-stories/US-GRD-008-ramas-protegidas-rutas-prohibidas.md) | Ningún agente cambia una rama protegida ni toca una ruta prohibida | Implementado (`implemented`) | #197 | — |
| [US-GRD-019](features/guardrails/user-stories/US-GRD-019-quien-ejecuto-y-a-nombre-de-quien.md) | El desarrollador ve quién ejecutó cada commit y a nombre de quién entró cuando no coinciden | Implementado (`implemented`) | #144, #147, #151, #175 | — |
| [US-GRP-020](features/motor-local/user-stories/US-GRP-020-carpetas-codigo-repos-descubiertos.md) | El desarrollador ve los repos que aparecen en sus carpetas de código sin tener que añadirlos uno a uno | Implementado (`implemented`) | #170, #153, #178 | — |
| [US-GRP-022](features/motor-local/user-stories/US-GRP-022-aceptar-descartar-repo-descubierto.md) | El desarrollador decide qué repos descubiertos se observan y los que descarta no vuelven a aparecer | Implementado (`implemented`) | #170 | — |
| [US-MCP-005](features/mcp/user-stories/US-MCP-005-respuestas-acotadas-y-seguras.md) | Un agente recibe respuestas acotadas que no pueden darle órdenes ni filtrar secretos | Implementado (`implemented`) | #157, #159 | — |
| [US-MCP-008](features/mcp/user-stories/US-MCP-008-snapshot-manual.md) | Un agente guarda por MCP un punto de recuperación antes de un cambio arriesgado | Implementado (`implemented`) | #213 | — |
| [US-TMC-006](features/time-machine/user-stories/US-TMC-006-timeline-que-cuando-quien.md) | El desarrollador sabe qué cambió en su repo, cuándo y quién lo hizo | Implementado (`implemented`) | #198 | — |
| [SPIKE-TMC-001](features/time-machine/technical-stories/SPIKE-TMC-001-repo-mediano-overhead.md) | Repo mediano de referencia y viabilidad del snapshot en menos de 200 ms | Hecho (`done`) | — | — |
| [TS-GRP-001](features/motor-local/technical-stories/TS-GRP-001-almacen-perfil.md) | Almacén de datos del motor en el perfil | Hecho (`done`) | — | — |
| [TS-GRP-002](features/motor-local/technical-stories/TS-GRP-002-lectura-git.md) | Capa de lectura de Git sin escrituras | Hecho (`done`) | — | — |
| [INF-GRP-001](features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md) | Arnés de verificación "repo intacto" en los tres SO | Implementado en parte (`partially-implemented`) | #30, #34, #68, #97, #109, #113, #163 | 3 |
| [INF-GRP-002](features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md) | Banco de medición de frescura y escala | Implementado en parte (`partially-implemented`) | #76, #98, #133 | 3 |
| [INF-TMC-001](features/time-machine/technical-stories/INF-TMC-001-arnes-caos-recuperable.md) | Arnés de caos y de garantías de los snapshots en los tres SO | Implementado en parte (`partially-implemented`) | — | — |
| [TS-GRP-004](features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md) | Canal local de clientes y contrato de mensajes | Implementado en parte (`partially-implemented`) | #36, #87, #123, #133, #158 | 3 |
| [US-CKP-025](features/cockpit/user-stories/US-CKP-025-tui-repo-no-observado.md) | La TUI ofrece observar el repo en el que se abre y, fuera de un repo, lleva a los ya observados | Implementado en parte (`partially-implemented`) | #165, #170 | 2 |
| [US-GRD-003](features/guardrails/user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md) | El desarrollador retira la protección y el repo queda exactamente como estaba | Implementado en parte (`partially-implemented`) | #193, #201 | — |
| [US-GRD-004](features/guardrails/user-stories/US-GRD-004-aviso-proteccion-inactiva.md) | El desarrollador se entera de que la protección de un repo dejó de estar activa | Implementado en parte (`partially-implemented`) | #201 | — |
| [US-GRD-005](features/guardrails/user-stories/US-GRD-005-registro-de-bloqueos.md) | El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo | Implementado en parte (`partially-implemented`) | #154, #168 | 3 |
| [US-GRD-018](features/guardrails/user-stories/US-GRD-018-autoria-commits-persona-y-agente.md) | Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo | Implementado en parte (`partially-implemented`) | #137, #141, #150, #152, #153, #154 | 3 |
| [US-TMC-009](features/time-machine/user-stories/US-TMC-009-restaurar-punto-timeline.md) | El desarrollador devuelve su worktree a cualquier punto del timeline | Implementado en parte (`partially-implemented`) | #212 | — |
| [TD-GRP-002](features/motor-local/technical-stories/TD-GRP-002-motor-bajo-rafaga.md) | Frescura y memoria del motor bajo una ráfaga de archivos | Listo (`ready`) | — | — |
| [TD-GRP-003](features/motor-local/technical-stories/TD-GRP-003-nfr04-maquina-referencia.md) | Presupuesto de NFR-04 sin gate automático: medirlo en una máquina de referencia | Listo (`ready`) | — | — |
| [TS-GRP-005](features/motor-local/technical-stories/TS-GRP-005-clases-trabajo-ahorro-energia.md) | Clases de trabajo del daemon y mecanismo de ahorro de energía | Listo (`ready`) | — | — |
| [INF-MCP-001](features/mcp/technical-stories/INF-MCP-001-corpus-seguridad-mcp.md) | Corpus de seguridad del MCP como suite de CI que bloquea el merge | Borrador (`draft`) | — | — |
| [SPIKE-CKP-001](features/cockpit/technical-stories/SPIKE-CKP-001-prediccion-5s.md) | Predicción de conflictos en ≤ 5 s p95 sin escribir en el repo: merge en memoria frente a almacén en el perfil | Borrador (`draft`) | — | — |
| [SPIKE-GRD-002](features/guardrails/technical-stories/SPIKE-GRD-002-factor-so-daemon.md) | Factor de autenticación del SO invocado desde el daemon en los tres SO | Borrador (`draft`) | — | — |
| [TS-CKP-001](features/cockpit/technical-stories/TS-CKP-001-predictor-conflictos.md) | Predictor de conflictos en el daemon: solape y conflicto previsto publicados para todos los clientes | Borrador (`draft`) | — | — |
| [TS-CKP-003](features/cockpit/technical-stories/TS-CKP-003-capa-cockpit-guardrails.md) | Capa cockpit en la decisión de Guardrails, ligada al git del ejecutor y registrada una sola vez | Borrador (`draft`) | — | — |
| [US-CKP-002](features/cockpit/user-stories/US-CKP-002-orden-atencion-terminadas.md) | La lista pone primero lo que pide atención y no se llena de sesiones viejas | Borrador (`draft`) | — | — |
| [US-CKP-003](features/cockpit/user-stories/US-CKP-003-estados-motor-conexion.md) | La TUI dice en qué estado está el motor y qué hacer en cada caso | Borrador (`draft`) | — | — |
| [US-CKP-004](features/cockpit/user-stories/US-CKP-004-selector-repo-preferencias.md) | El desarrollador cambia de repo y la TUI recuerda cómo la dejó | Borrador (`draft`) | — | — |
| [US-CKP-005](features/cockpit/user-stories/US-CKP-005-terminal-pequena-sin-color.md) | La TUI se puede usar en una terminal pequeña, sin color o en ASCII | Borrador (`draft`) | — | — |
| [US-CKP-006](features/cockpit/user-stories/US-CKP-006-conflictos-previstos.md) | El desarrollador ve qué agentes van a chocar antes de hacer merge | Borrador (`draft`) | — | — |
| [US-CKP-007](features/cockpit/user-stories/US-CKP-007-frescura-prediccion.md) | Una predicción vieja nunca se presenta como actual | Borrador (`draft`) | — | — |
| [US-CKP-008](features/cockpit/user-stories/US-CKP-008-alerta-conflicto-nuevo.md) | El desarrollador se entera en la TUI cuando aparece un conflicto previsto nuevo | Borrador (`draft`) | — | — |
| [US-CKP-009](features/cockpit/user-stories/US-CKP-009-base-pendiente.md) | Con la rama base sin confirmar, la predicción contra la base queda pendiente | Borrador (`draft`) | — | — |
| [US-CKP-010](features/cockpit/user-stories/US-CKP-010-kpi-deteccion.md) | El desarrollador sabe qué parte de los conflictos reales se vio antes de ocurrir | Borrador (`draft`) | — | — |
| [US-CKP-011](features/cockpit/user-stories/US-CKP-011-cli-solo-lectura.md) | El desarrollador consulta la flota y los conflictos desde la línea de comandos | Borrador (`draft`) | — | — |
| [US-CKP-012](features/cockpit/user-stories/US-CKP-012-ver-diff.md) | El desarrollador revisa lo que un agente integraría antes de hacer merge | Borrador (`draft`) | — | — |
| [US-CKP-013](features/cockpit/user-stories/US-CKP-013-abrir-en-editor.md) | El desarrollador abre el worktree de un agente en su editor | Borrador (`draft`) | — | — |
| [US-CKP-014](features/cockpit/user-stories/US-CKP-014-integrar-rama-agente.md) | El desarrollador integra la rama de un agente con una tecla y puede deshacerlo | Borrador (`draft`) | — | — |
| [US-CKP-015](features/cockpit/user-stories/US-CKP-015-rebasar-rama-agente.md) | El desarrollador pone al día la rama de un agente sobre la base | Borrador (`draft`) | — | — |
| [US-CKP-016](features/cockpit/user-stories/US-CKP-016-operacion-detenida.md) | Un merge o rebase que choca queda detenido y el desarrollador decide cómo salir | Borrador (`draft`) | — | — |
| [US-CKP-017](features/cockpit/user-stories/US-CKP-017-descartar-worktree.md) | El desarrollador descarta el trabajo de un agente sin miedo a perderlo | Borrador (`draft`) | — | — |
| [US-CKP-018](features/cockpit/user-stories/US-CKP-018-crear-worktree.md) | El desarrollador prepara un worktree para un agente nuevo | Borrador (`draft`) | — | — |
| [US-CKP-019](features/cockpit/user-stories/US-CKP-019-denegada-excepcion-consciente.md) | El desarrollador entiende por qué Guardrails frena una acción y puede hacer una excepción consciente | Borrador (`draft`) | — | — |
| [US-CKP-020](features/cockpit/user-stories/US-CKP-020-trabajo-de-otro-actor.md) | Tocar el trabajo de otro actor exige confirmar el plan concreto | Borrador (`draft`) | — | — |
| [US-CKP-021](features/cockpit/user-stories/US-CKP-021-historial-aviso-purga.md) | El desarrollador ve en la TUI el historial de operaciones y el aviso de purga | Borrador (`draft`) | — | — |
| [US-CKP-022](features/cockpit/user-stories/US-CKP-022-grafo-carriles.md) | El desarrollador ve crecer la rama de cada agente sobre la base | Borrador (`draft`) | — | — |
| [US-CKP-024](features/cockpit/user-stories/US-CKP-024-integrar-casos-limite.md) | Integrar sigue siendo seguro cuando el estado cambia, choca o la base no está sacada | Borrador (`draft`) | — | — |
| [US-GRD-006](features/guardrails/user-stories/US-GRD-006-excepcion-consciente.md) | El desarrollador hace a conciencia una operación prohibida y queda constancia | Borrador (`draft`) | — | — |
| [US-GRD-007](features/guardrails/user-stories/US-GRD-007-permisos-por-operacion.md) | El equipo decide qué operaciones de Git se permiten, se deniegan o piden confirmación | Borrador (`draft`) | — | — |
| [US-GRD-009](features/guardrails/user-stories/US-GRD-009-tamano-diff-formato-commit.md) | Los agentes entregan commits pequeños y con el formato que exige el equipo | Borrador (`draft`) | — | — |
| [US-GRD-010](features/guardrails/user-stories/US-GRD-010-endurecer-sin-relajar.md) | El desarrollador endurece las reglas en su máquina sin poder relajar las del equipo | Borrador (`draft`) | — | — |
| [US-GRD-011](features/guardrails/user-stories/US-GRD-011-configuracion-ilegible.md) | Una configuración rota no deja pasar las operaciones peligrosas | Borrador (`draft`) | — | — |
| [US-GRD-012](features/guardrails/user-stories/US-GRD-012-agente-no-relaja-configuracion.md) | Un agente no puede relajar las reglas del equipo cambiando su configuración | Borrador (`draft`) | — | — |
| [US-GRD-013](features/guardrails/user-stories/US-GRD-013-comando-edicion-configuracion.md) | El desarrollador cambia la configuración con un comando sin perder lo que editó a mano | Borrador (`draft`) | — | — |
| [US-GRD-014](features/guardrails/user-stories/US-GRD-014-rama-base-del-equipo.md) | El equipo fija la rama base del repo y Guardrails la protege | Borrador (`draft`) | — | — |
| [US-GRD-015](features/guardrails/user-stories/US-GRD-015-cola-de-confirmacion.md) | El desarrollador aprueba o rechaza las acciones de riesgo que un agente deja en espera | Borrador (`draft`) | — | — |
| [US-GRD-016](features/guardrails/user-stories/US-GRD-016-misma-decision-por-mcp.md) | Un agente que usa las herramientas MCP recibe la misma decisión que con Git directo | Borrador (`draft`) | — | — |
| [US-GRD-017](features/guardrails/user-stories/US-GRD-017-sin-snapshot-no-se-ejecuta.md) | Una operación destructiva permitida no se ejecuta sin un punto de recuperación | Borrador (`draft`) | — | — |
| [US-GRP-003](features/motor-local/user-stories/US-GRP-003-estados-especiales-no-disponible.md) | El desarrollador sabe qué worktree está en un estado especial o ya no existe | Borrador (`draft`) | — | — |
| [US-GRP-005](features/motor-local/user-stories/US-GRP-005-hueco-sin-atribuir.md) | El desarrollador distingue lo que el motor no vio ocurrir | Borrador (`draft`) | — | — |
| [US-GRP-006](features/motor-local/user-stories/US-GRP-006-retirar-y-volver-a-anadir.md) | El desarrollador recupera el historial de un repo que retiró y volvió a añadir | Borrador (`draft`) | — | — |
| [US-GRP-008](features/motor-local/user-stories/US-GRP-008-editor-humano-sin-atribuir.md) | El trabajo que el desarrollador hace en su editor nunca se atribuye a Claude Code | Borrador (`draft`) | — | — |
| [US-GRP-010](features/motor-local/user-stories/US-GRP-010-corregir-atribucion.md) | El desarrollador corrige una atribución automática equivocada | Borrador (`draft`) | — | — |
| [US-GRP-011](features/motor-local/user-stories/US-GRP-011-worktree-compartido.md) | El desarrollador ve todas las sesiones de un worktree compartido | Borrador (`draft`) | — | — |
| [US-GRP-013](features/motor-local/user-stories/US-GRP-013-umbral-inactividad-por-repo.md) | El desarrollador ajusta para un repo cuándo una sesión pasa a inactiva | Borrador (`draft`) | — | — |
| [US-GRP-016](features/motor-local/user-stories/US-GRP-016-rama-base-configuracion-equipo.md) | La rama base la define la configuración del equipo | Borrador (`draft`) | — | — |
| [US-MCP-004](features/mcp/user-stories/US-MCP-004-status-completo-y-no-disponible.md) | Un agente sabe por MCP quién más trabaja en su repo, contra qué base y con qué protección | Borrador (`draft`) | — | — |
| [US-MCP-006](features/mcp/user-stories/US-MCP-006-registrar-agente.md) | Un agente sin soporte completo se declara por MCP y su trabajo queda a su nombre | Borrador (`draft`) | — | — |
| [US-MCP-007](features/mcp/user-stories/US-MCP-007-confused-deputy.md) | Un hook lanzado por una escritura de un agente no puede usar los poderes del desarrollador | Borrador (`draft`) | — | — |
| [US-MCP-009](features/mcp/user-stories/US-MCP-009-safe-commit.md) | Un agente commitea por MCP con las reglas del repo y con un punto de recuperación previo | Borrador (`draft`) | — | — |
| [US-MCP-010](features/mcp/user-stories/US-MCP-010-safe-commit-rutas.md) | Un agente commitea solo los archivos que nombra y nunca fuera de su worktree | Borrador (`draft`) | — | — |
| [US-MCP-011](features/mcp/user-stories/US-MCP-011-escritura-en-el-worktree-correcto.md) | La escritura de un agente cae en el worktree que espera, o no ocurre | Borrador (`draft`) | — | — |
| [US-MCP-012](features/mcp/user-stories/US-MCP-012-undo-de-lo-propio.md) | Un agente deshace por MCP su última operación sin tocar el trabajo de otros | Borrador (`draft`) | — | — |
| [US-MCP-013](features/mcp/user-stories/US-MCP-013-confirmacion-sin-cola.md) | Un agente sabe que una acción necesita al desarrollador y a quién acudir | Borrador (`draft`) | — | — |
| [US-MCP-014](features/mcp/user-stories/US-MCP-014-confirmacion-pendiente-en-cola.md) | La acción de riesgo de un agente queda en espera del desarrollador sin bloquear al agente | Borrador (`draft`) | — | — |
| [US-MCP-015](features/mcp/user-stories/US-MCP-015-medicion-del-mcp.md) | El desarrollador mide cuánto paró y cuánto recuperó el MCP | Borrador (`draft`) | — | — |
| [US-MCP-016](features/mcp/user-stories/US-MCP-016-check-conflicts.md) | Un agente sabe con quién va a chocar antes de que choque | Borrador (`draft`) | — | — |
| [US-MCP-017](features/mcp/user-stories/US-MCP-017-explain-history.md) | Un agente entiende qué pasó en su repo sin ver mensajes ni contenido | Borrador (`draft`) | — | — |
| [US-MCP-018](features/mcp/user-stories/US-MCP-018-safe-rebase-atomico.md) | Un agente se pone al día con la rama base y, si choca, su rama queda como estaba | Borrador (`draft`) | — | — |
| [US-MCP-019](features/mcp/user-stories/US-MCP-019-create-worktree.md) | Un agente prepara un worktree nuevo desde la rama base para otro trabajo | Borrador (`draft`) | — | — |
| [US-TMC-003](features/time-machine/user-stories/US-TMC-003-redo.md) | El desarrollador rehace lo que deshizo por error | Borrador (`draft`) | — | — |
| [US-TMC-005](features/time-machine/user-stories/US-TMC-005-snapshot-previo-hooks-guardrails.md) | Las operaciones de Git crudo tienen punto previo cuando el repo usa los hooks de Guardrails | Borrador (`draft`) | — | — |
| [US-TMC-007](features/time-machine/user-stories/US-TMC-007-timeline-filtros-huecos.md) | El desarrollador filtra el timeline por worktree, agente o periodo y ve lo que no se observó | Borrador (`draft`) | — | — |
| [US-TMC-008](features/time-machine/user-stories/US-TMC-008-timeline-atribucion-vigente.md) | El timeline refleja las correcciones de atribución sin reescribir quién deshizo qué | Borrador (`draft`) | — | — |
| [US-TMC-010](features/time-machine/user-stories/US-TMC-010-undo-since.md) | El desarrollador deshace todo lo ocurrido en su worktree en los últimos minutos | Borrador (`draft`) | — | — |
| [US-TMC-011](features/time-machine/user-stories/US-TMC-011-undo-por-agente.md) | El desarrollador deshace solo lo que hizo un agente en un periodo | Borrador (`draft`) | — | — |
| [US-TMC-012](features/time-machine/user-stories/US-TMC-012-solape-otro-actor.md) | Un undo nunca sobrescribe trabajo posterior de otro actor | Borrador (`draft`) | — | — |
| [US-TMC-013](features/time-machine/user-stories/US-TMC-013-permisos-solicitante.md) | Un agente no puede deshacer trabajo ajeno, aunque lance la CLI desde su propia shell | Borrador (`draft`) | — | — |
| [US-TMC-014](features/time-machine/user-stories/US-TMC-014-undo-ya-empujado.md) | El desarrollador sabe cuándo lo que deshizo sigue en el remoto | Borrador (`draft`) | — | — |
| [US-TMC-015](features/time-machine/user-stories/US-TMC-015-operacion-git-en-curso.md) | El desarrollador no rompe un rebase o un merge a medias al deshacer o restaurar | Borrador (`draft`) | — | — |
| [US-TMC-016](features/time-machine/user-stories/US-TMC-016-retencion-por-defecto.md) | Los snapshots no llenan el disco y nunca se pierde el último punto antes de una operación destructiva | Borrador (`draft`) | — | — |
| [US-TMC-018](features/time-machine/user-stories/US-TMC-018-garantias-snapshots.md) | Los snapshots no se publican, no se pierden con el mantenimiento de Git y ningún agente los altera | Borrador (`draft`) | — | — |
| [US-TMC-019](features/time-machine/user-stories/US-TMC-019-robustez-interrupcion.md) | El repo sigue recuperable aunque GitRaptor muera a mitad de un snapshot o de un undo | Borrador (`draft`) | — | — |
| [US-TMC-020](features/time-machine/user-stories/US-TMC-020-overhead-snapshot.md) | El desarrollador y sus agentes no notan el coste de los snapshots | Borrador (`draft`) | — | — |
| [US-TMC-022](features/time-machine/user-stories/US-TMC-022-tope-disco.md) | La Time Machine nunca pasa del tope de disco que fijé | Borrador (`draft`) | — | — |
| [US-CKP-023](features/cockpit/user-stories/US-CKP-023-cola-confirmacion.md) | El desarrollador aprueba o rechaza desde la TUI las acciones que un agente deja en espera | Bloqueado (`blocked`) | — | — |

### M3

3 fichas: 3 implementado en parte.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [SPIKE-GRD-001](features/guardrails/technical-stories/SPIKE-GRD-001-interceptabilidad-hooks.md) | Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO | Implementado en parte (`partially-implemented`) | #22, #25, #121 | 2 |
| [SPIKE-GRP-002](features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md) | Viabilidad del observador de cambios a escala en los tres SO | Implementado en parte (`partially-implemented`) | #17, #23 | 2 |
| [TD-GRP-001](features/motor-local/technical-stories/TD-GRP-001-acl-windows.md) | Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06) | Implementado en parte (`partially-implemented`) | #72 | 2 |

### M4

4 fichas: 2 implementado en parte, 2 borrador.

| Id | Título | Estado | Implementado en | Pendientes |
|---|---|---|---|---|
| [INF-GRP-003](features/motor-local/technical-stories/INF-GRP-003-pipeline-release.md) | Pipeline de release: 6 targets, firma, checksums, SBOM y borrador de GitHub Release | Implementado en parte (`partially-implemented`) | #51 | 3 |
| [INF-GRP-004](features/motor-local/technical-stories/INF-GRP-004-canales-distribucion.md) | Canales de distribución: Homebrew, winget, npm y scripts de instalación verificados | Implementado en parte (`partially-implemented`) | #51 | 2 |
| [US-GRP-014](features/motor-local/user-stories/US-GRP-014-git-ausente-o-antiguo.md) | El desarrollador sabe qué Git le falta y el motor empieza solo cuando lo instala | Borrador (`draft`) | — | — |
| [US-GRP-015](features/motor-local/user-stories/US-GRP-015-primer-repo-maquina-nueva.md) | El desarrollador recién instalado sabe cómo añadir su primer repo | Borrador (`draft`) | — | — |

### Sin hito

7 fichas sin hito asignado.

## Implementadas en parte: lo que falta

### [US-CKP-025](features/cockpit/user-stories/US-CKP-025-tui-repo-no-observado.md) — #165, #170

La TUI ofrece observar el repo en el que se abre y, fuera de un repo, lleva a los ya observados

- Niveles en la TUI: mostrar el `tier` de cada repo y "reconciliando" al abrir un repo dormido (ADR-GRP-010 N4, RES-12). El escenario 6 (repos descubiertos) lo cerró el #170.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [SPIKE-GRD-001](features/guardrails/technical-stories/SPIKE-GRD-001-interceptabilidad-hooks.md) — #22, #25, #121

Interceptabilidad, coexistencia y coste de la capa de hooks en los tres SO

- Matriz en Linux y Windows (incluido el coste con el dispatcher nativo) y los casos sin verificar del § 11 de los resultados.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [US-GRD-003](features/guardrails/user-stories/US-GRD-003-retirar-proteccion-sin-rastro.md) — #193, #201

El desarrollador retira la protección y el repo queda exactamente como estaba

- *La ficha no lista los pendientes en "Estado de la implementación".*

### [US-GRD-004](features/guardrails/user-stories/US-GRD-004-aviso-proteccion-inactiva.md) — #201

El desarrollador se entera de que la protección de un repo dejó de estar activa

- *La ficha no lista los pendientes en "Estado de la implementación".*

### [US-GRD-005](features/guardrails/user-stories/US-GRD-005-registro-de-bloqueos.md) — #154, #168

El desarrollador cuenta las acciones peligrosas que Guardrails bloqueó en cada repo

- El escenario de las entradas en modo degradado: spool y `spool-unverified` (segunda entrega, TS a crear).
- Entradas `request` y `exception*` (US-GRD-015 y US-GRD-006) y `protection-state` (US-GRD-003).
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [US-GRD-018](features/guardrails/user-stories/US-GRD-018-autoria-commits-persona-y-agente.md) — #137, #141, #150, #152, #153, #154

Cada commit entra a nombre de la persona y deja constancia del agente que lo hizo, con la exigencia que fija el equipo

- `detected_and_registered_agents_are_the_actor`: solo se cubre el agente detectado, no el registrado (DS § 7).
- Nivel local (`settings.local.json`): US-GRP-013. `flexible` de extremo a extremo con un suelo de equipo: US-GRD-014.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [INF-GRP-001](features/motor-local/technical-stories/INF-GRP-001-arnes-repo-intacto.md) — #30, #34, #68, #97, #109, #113, #163

Arnés de verificación "repo intacto" en los tres SO

- Auditoría dinámica de `exec` parcial: ETW (Windows) sin implementar y eslogger (macOS) sin verificar; la Validación 7 de ADR-GRP-009 sigue abierta.
- Canario en Windows y repo de otro uid real.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [INF-GRP-002](features/motor-local/technical-stories/INF-GRP-002-banco-frescura-escala.md) — #76, #98, #133

Banco de medición de frescura y escala

- ADR-GRP-015: ventana de 10 min con la Time Machine activa (RES-01), RES-03, gate de inotify (RES-04), RES-05 y RES-07.
- Runner dedicado para el gate de latencia (TD-GRP-003) y escenario `tiered-scale`.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [INF-GRP-003](features/motor-local/technical-stories/INF-GRP-003-pipeline-release.md) — #51

Pipeline de release: 6 targets, firma, checksums, SBOM y borrador de GitHub Release

- Pasos humanos antes de publicar: reservar los nombres en npm, crear el tap, configurar secretos y variables y activar las releases inmutables.
- Borrar a mano los tags de prueba `v0.0.0` y `v0.0.0-test.1`.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [INF-GRP-004](features/motor-local/technical-stories/INF-GRP-004-canales-distribucion.md) — #51

Canales de distribución: Homebrew, winget, npm y scripts de instalación verificados

- Pasos humanos antes de publicar (npm, tap, secretos, releases inmutables); `winget validate`/`brew audit` antes de la primera publicación.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [SPIKE-GRP-001](features/motor-local/technical-stories/SPIKE-GRP-001-precision-deteccion.md) — #80, #155, #169

Precisión de la detección de Claude Code en dogfooding

- Validación parcial: señales S1, S3 y S4 con suites guionizadas en macOS.
- Medición en el dogfooding real (2 semanas, al menos 50 sesiones) y Research Brief: criterio 2 de M1.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [SPIKE-GRP-002](features/motor-local/technical-stories/SPIKE-GRP-002-viabilidad-observador.md) — #17, #23

Viabilidad del observador de cambios a escala en los tres SO

- Medición en Linux y Windows.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [TD-GRP-001](features/motor-local/technical-stories/TD-GRP-001-acl-windows.md) — #72

Verificación de ACL en Windows: git.exe (SEC-10) y perfil (SEC-06)

- Ruta de instalación de Git leída del registro (ADR-GRP-009 § 4).
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [TS-GRP-004](features/motor-local/technical-stories/TS-GRP-004-canal-clientes.md) — #36, #87, #123, #133, #158

Canal local de clientes y contrato de mensajes

- N8 a N11 de ADR-CKP-003 § 4 (Should).
- Retención de la auditoría de comandos reservados.
- Linux y Windows: *Pendiente: etapa de validación multiplataforma* ([`xplat-pendientes.md`](../architecture/xplat-pendientes.md)).

### [INF-TMC-001](features/time-machine/technical-stories/INF-TMC-001-arnes-caos-recuperable.md)

Arnés de caos y de garantías de los snapshots en los tres SO

- *La ficha no lista los pendientes en "Estado de la implementación".*

### [US-TMC-009](features/time-machine/user-stories/US-TMC-009-restaurar-punto-timeline.md) — #212

El desarrollador devuelve su worktree a cualquier punto del timeline

- *La ficha no lista los pendientes en "Estado de la implementación".*
