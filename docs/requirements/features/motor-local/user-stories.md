---
mode: draft
generated: 2026-10-03T00:00Z
updated: 2026-10-04
generator: product-owner
total_artifacts: 16
expanded: 16
approved: 0
blocked: []
---

# User Stories — INDEX: Motor local

> Outline generado en modo Draft y expandido con el equivalente a `/aadd-expand --all` (2026-10-03). Cada historia vive en su archivo de `user-stories/`; esta tabla solo enlaza y marca el Status.

---

## Contexto del Feature

**Feature**: Motor local (F-001-01)
**Epic**: E-001 — MVP Fase 1: Cockpit + Time Machine + Guardrails (CLI/TUI + MCP)
**Prioridad**: Alta
**Estado**: Listo para Desarrollo, sin historias bloqueadas: US-GRP-013 y US-GRP-016 se desbloquearon el 2026-10-04 al aceptarse ADR-GRP-007, que cierra P8 (requerimiento aprobado por Rene Bonilla el 2026-10-03; alcance ajustado por Q32 y Q33-Q36 el mismo día)

**Enlace a contexto completo**: [`context.md`](./context.md) (CTX-GRP-001)
**Reglas de negocio**: [`business-rules.md`](./business-rules.md) (BR-GRP-001, 22 reglas) · **Diseño**: no aplica (el motor no tiene superficie propia)

---

## Análisis de División (Sistema de 4 Pasos)

- **Paso 1 — Flujo**: actor principal = desarrollador orquestador; resultado buscado = saber qué agente toca qué worktree y en qué estado está, sin reconstruirlo a mano; camino mínimo = añadir un repo → ver sus worktrees → ver los cambios en vivo.
- **Paso 2 — Puntos de resultado**: cada historia deja algo que se puede consultar y comprobar desde fuera (un estado, un evento, una atribución con su origen).
- **Paso 3 — Patrón aplicado**: camino feliz primero (esqueleto andante US-GRP-001 y 002), luego capacidad acumulable (continuidad, atribución, configuración, primer uso) y corte por actor en la atribución: Claude Code (US-GRP-007), el editor del humano (US-GRP-008) y cualquier otro agente por registro (US-GRP-009).
- **Paso 4 — Filtro**: todas pasan (usuario real · resultado observable · cabe en una rama corta de un agente · comportamiento, no código).

> **Cómo se verifica cada historia** (decisión de Rene Bonilla, 2026-10-03): consultando el estado que expone el motor, que no tiene UI propia. La presentación final (textos, estados vacíos, vista en vivo) es del Cockpit (F-001-02) y de la CLI. Las historias las revisan Rene Bonilla y el orquestador juntos.
>
> **Formato para la flota**: historias compactas como contratos verificables para agentes (Q32 y enfoque de Rene Bonilla): frontmatter, título, historia y valor en una línea, reglas cubiertas, dependencias y escenarios declarativos que se pueden convertir en tests. Sin certificación ni requisitos técnicos: lo técnico va en el plan y las dev specs.
>
> **Transversales (no son historias)**: el mismo comportamiento en Windows, macOS y Linux (BR-03, NFR-06) y "cero escrituras en el repo" (BR-CONS-001) se exigen en todos los escenarios de todas las historias. Cómo se comprueban lo define el plan técnico.

---

## Outline

| ID | Título | Resumen (1 línea) | Status |
|----|--------|-------------------|--------|
| [US-GRP-001](./user-stories/US-GRP-001-estado-worktrees-repo-anadido.md) | El desarrollador ve el estado de cada worktree del repo que añadió | Desarrollador quiere añadir un repo y ver rama, cambios y archivos modificados de sus worktrees sin tocar el repo | expanded |
| [US-GRP-002](./user-stories/US-GRP-002-cambios-eventos-en-vivo.md) | El desarrollador ve los cambios y eventos de Git de sus worktrees casi al instante | Desarrollador quiere ver en vivo cambios y eventos (commit, rama, rebase, push) con su momento y actor | expanded |
| [US-GRP-003](./user-stories/US-GRP-003-estados-especiales-no-disponible.md) | El desarrollador sabe qué worktree está en un estado especial o ya no existe | Desarrollador quiere ver estados especiales y worktrees no disponibles sin perder la vista del resto | expanded |
| [US-GRP-004](./user-stories/US-GRP-004-observacion-continua.md) | El desarrollador encuentra lo ocurrido aunque no tuviera GitRaptor abierto | Desarrollador quiere que la actividad se capture sin superficie abierta y sobreviva a reinicios | expanded |
| [US-GRP-005](./user-stories/US-GRP-005-hueco-sin-atribuir.md) | El desarrollador distingue lo que el motor no vio ocurrir | Desarrollador quiere que los cambios de un hueco o tras perder el perfil queden "sin atribuir" | expanded |
| [US-GRP-006](./user-stories/US-GRP-006-retirar-y-volver-a-anadir.md) | El desarrollador recupera el historial de un repo que retiró y volvió a añadir | Desarrollador quiere retirar un repo sin perder sus datos y recuperarlos al volver a añadirlo | expanded |
| [US-GRP-007](./user-stories/US-GRP-007-sesiones-claude-code.md) | El desarrollador sabe qué sesión de Claude Code trabaja en cada worktree y si sigue activa | Desarrollador quiere detectar sesiones de Claude Code con su estado sin registrarlas a mano | expanded |
| [US-GRP-008](./user-stories/US-GRP-008-editor-humano-sin-atribuir.md) | El trabajo que el desarrollador hace en su editor nunca se atribuye a Claude Code | Desarrollador quiere que lo que edita en Cursor u otro editor quede fuera de cualquier agente y lo dudoso sin atribuir | expanded |
| [US-GRP-009](./user-stories/US-GRP-009-registro-explicito-agente.md) | El desarrollador o el agente declaran qué agente trabaja en un worktree | Desarrollador o agente quieren registrar cualquier agente (Codex, Cursor como "otro agente") para que su trabajo se le atribuya | expanded |
| [US-GRP-010](./user-stories/US-GRP-010-corregir-atribucion.md) | El desarrollador corrige una atribución automática equivocada | Desarrollador quiere que su corrección reemplace una detección errónea, sin dejar una segunda sesión | expanded |
| [US-GRP-011](./user-stories/US-GRP-011-worktree-compartido.md) | El desarrollador ve todas las sesiones de un worktree compartido | Desarrollador quiere que registrar otro agente añada su sesión y el worktree figure como compartido | expanded |
| [US-GRP-012](./user-stories/US-GRP-012-rama-base-main.md) | El desarrollador ve el ahead/behind de cada worktree contra la rama base del repo | Desarrollador quiere ahead/behind contra `main` (provisional) al día y sin tocar el remoto | expanded |
| [US-GRP-013](./user-stories/US-GRP-013-umbral-inactividad-por-repo.md) | El desarrollador ajusta para un repo cuándo una sesión pasa a inactiva | Desarrollador quiere fijar su umbral de inactividad por repo sin imponerlo al equipo | expanded |
| [US-GRP-014](./user-stories/US-GRP-014-git-ausente-o-antiguo.md) | El desarrollador sabe qué Git le falta y el motor empieza solo cuando lo instala | Desarrollador quiere que sin Git 2.38 el motor avise, espere y arranque solo al detectarlo | expanded |
| [US-GRP-015](./user-stories/US-GRP-015-primer-repo-maquina-nueva.md) | El desarrollador recién instalado sabe cómo añadir su primer repo | Desarrollador quiere una guía sin repos y, en máquina nueva, empezar de cero sin historia atribuida | expanded |
| [US-GRP-016](./user-stories/US-GRP-016-rama-base-configuracion-equipo.md) | La rama base la define la configuración del equipo | Desarrollador quiere el ahead/behind contra la rama base del equipo, sin que un ajuste personal la cambie | expanded |

---

## Mapa para la flota

> Un agente por historia y por worktree (D3). Prioridad MoSCoW. "Depende de" = historias que deben estar integradas antes de empezar. Dependencias externas = otras features o decisiones pendientes; no se crean historias de otras features aquí.

| ID | Reglas cubiertas | Depende de | Externas | Prioridad | Valor en una línea |
|----|------------------|------------|----------|-----------|--------------------|
| US-GRP-001 | BR-AUTH-001 (incluido: un agente no cambia los repos, Q40), BR-CONS-001, BR-AUTH-002 | — | MCP F-001-05 (canal del agente en un escenario; no bloqueante) | Must | Deja de reconstruir el estado a mano con `git status` en cada directorio |
| US-GRP-002 | BR-CONS-003 ("sin atribuir" por defecto) | US-GRP-001 | Cockpit F-001-02 (presupuesto NFR-04 compartido) | Must | Ve lo que pasa mientras pasa |
| US-GRP-003 | BR-EDGE-001, BR-EDGE-002 | US-GRP-001, US-GRP-009 | — | Must | Un worktree borrado o en rebase no rompe ni falsea la vista |
| US-GRP-004 | BR-CONS-005 | US-GRP-002, US-GRP-007, US-GRP-009 | — | Must | La Time Machine tiene historia sin huecos |
| US-GRP-005 | BR-EDGE-005 | US-GRP-004, US-GRP-009 | — | Must | Un deshacer por agente nunca alcanza lo que nadie vio |
| US-GRP-006 | BR-AUTH-001, BR-EDGE-005 | US-GRP-004, US-GRP-005, US-GRP-009 | MCP F-001-05 (canal del agente en un escenario; no bloqueante) | Should | Retirar un repo no cuesta su historial |
| US-GRP-007 | BR-WF-001, BR-TIME-001 (5 min por defecto), BR-CONS-003, BR-AUTH-002, BR-EDGE-006 | US-GRP-002 | Spike (c) del BRD (mecanismo de detección) | Must | Sabe qué hace cada Claude Code sin preguntarlo (único soporte completo del MVP, Q32) |
| US-GRP-008 | BR-EDGE-004, BR-EDGE-003, BR-CONS-003 | US-GRP-007, US-GRP-009 | Spike (c) del BRD (riesgo R2) | Must | El trabajo humano en su editor queda protegido del deshacer por agente |
| US-GRP-009 | BR-VAL-001, BR-VAL-002, BR-CONS-003, BR-WF-001, BR-CONS-004 (confirmar sesión ya detectada, Q39) | US-GRP-002, US-GRP-007 | MCP F-001-05 (auto-registro del agente) | Must | Cualquier agente, incluidos Codex y Cursor como "otro agente", queda observado |
| US-GRP-010 | BR-CONS-002 (Q33, Q37, Q38), BR-CONS-003, BR-VAL-002, BR-AUTH-001 (el agente no corrige), BR-CONS-005 | US-GRP-004, US-GRP-007, US-GRP-009 | — | Must | Una detección errónea se reemplaza en un paso, también hacia atrás en la sesión (Q33, Q37) |
| US-GRP-011 | BR-CONS-004, BR-CONS-005 | US-GRP-004, US-GRP-007, US-GRP-009 | — | Should | Registrar otro agente lo suma y ve cuándo dos agentes pisan el mismo worktree (Q33) |
| US-GRP-012 | BR-CONS-006 (`main` provisional; única dueña del ahead/behind; rama base inexistente, Q42), BR-CONS-001 (Q12) | US-GRP-001 | — | Should | Ahead/behind fiable desde ya |
| US-GRP-013 | BR-TIME-001 (umbral configurado), BR-CONS-007 | US-GRP-007 | — (P8 cerrada por ADR-GRP-007, aceptado el 2026-10-04) | Should | El estado de sesión se ajusta a su ritmo en cada repo |
| US-GRP-014 | BR-VAL-003, BR-WF-002, BR-EDGE-005 | US-GRP-001, US-GRP-002, US-GRP-005, US-GRP-009, US-GRP-015 | Cockpit F-001-02 y CLI (presentación del aviso) | Must | Una máquina nueva no parece una herramienta rota |
| US-GRP-015 | BR-WF-002, BR-EDGE-007 | US-GRP-001, US-GRP-002 | Cockpit F-001-02 y CLI (estado vacío guiado) | Should | El primer minuto termina con un repo observado |
| US-GRP-016 | BR-CONS-006 (equipo), BR-CONS-007, BR-EDGE-007 (rama base del equipo) | US-GRP-012, US-GRP-013, TS-GRD-001 (Guardrails) | — (Q36 satisfecha: ADR-GRP-007 y ADR-GRD-004 aceptados el 2026-10-04) | Should | Todo el equipo mide contra la misma rama base |

> **Cambios de dependencias (2026-10-03, segunda pasada del Artifact Judge)**: US-GRP-004 añade 007 y 009 (sesiones y registros que sobreviven al reinicio); US-GRP-006 añade 005 (lo ocurrido mientras estuvo retirado es un hueco) y 009 (atribuciones recuperadas), y su dependencia del MCP queda como no bloqueante; US-GRP-009 depende de 007 y US-GRP-014 de 015 para que no corran en paralelo sobre el mismo modelo; US-GRP-005 ya no exige el estado "Sin repos" (solo la lista vacía). US-GRP-001 deja BR-CONS-006: el ahead/behind es solo de US-GRP-012. US-GRP-013 pasa a bloqueada por P8 y el umbral por defecto queda en US-GRP-007. Nueva US-GRP-016, bloqueada (Q36).
>
> **Cambios de dependencias (2026-10-03, cierre de las RESERVAS del Artifact Judge)**: US-GRP-008 depende de 009 (en serie por el contrato de "quién hizo un evento"); US-GRP-010 y US-GRP-011 añaden 004 (la corrección y lo compartido sobreviven al reinicio del motor); US-GRP-014 añade 005 y 009 (lo ocurrido mientras espera Git es un hueco, con un agente registrado en el escenario); US-GRP-016 depende de 013 (las dos bloqueadas por P8, en ese orden). US-GRP-001 asume el rechazo a un agente que cambia los repos (Q40).

### Orden y paralelismo

- **Esqueleto andante** (en serie, primero): US-GRP-001 → US-GRP-002. Con ambas se observa un repo de punta a punta.
- **Ola 1** (en paralelo tras el esqueleto): US-GRP-007, US-GRP-012 y US-GRP-015.
- **Ola 2**: US-GRP-009 y US-GRP-013 (tras 007).
- **Ola 3**: US-GRP-003, US-GRP-004 y US-GRP-008 (tras 009) y US-GRP-016 (tras 012, 013 y TS-GRD-001 de Guardrails).
- **Ola 4**: US-GRP-005, US-GRP-010 y US-GRP-011 (tras 004).
- **Ola 5**: US-GRP-006 y US-GRP-014 (tras 005).
- **Desbloqueadas el 2026-10-04** (ADR-GRP-007 aceptado, cierra P8): US-GRP-013 → US-GRP-016, en ese orden; US-GRP-016 espera además a TS-GRD-001 (Guardrails).

> **Secuencias por contrato compartido**: US-GRP-007 → US-GRP-009 (modelo de sesión: estados, origen, presencia), US-GRP-009 → US-GRP-008 (quién hizo un evento: agente con su origen o "sin atribuir"), US-GRP-015 → US-GRP-014 (estados del motor de BR-WF-002) y US-GRP-013 → US-GRP-016 (lectura de la configuración en tres niveles) van en serie, no en paralelo. US-GRP-010 y US-GRP-011 corren en paralelo pero tocan la misma regla de sesiones por worktree (corregir frente a añadir, Q33). En todos los casos el contrato compartido lo fija la Dev Spec de la historia que va primero (007, 009, 015, 013 y, para 010/011, 009).
>
> **Ruta crítica**: 001 → 002 → 007 → 009 → 004 → 005 → 006 / 014. Son cinco olas tras el esqueleto; 008 y 014 se movieron de la ola 2 sin alargarla, porque ya cuelgan de 009 y de 005.

---

## Cobertura de reglas (regla → historias)

| Regla | Historias | Regla | Historias |
|-------|-----------|-------|-----------|
| BR-VAL-001 | US-GRP-009 | BR-CONS-005 | US-GRP-004 (+ persistencia en US-GRP-010 y US-GRP-011) |
| BR-VAL-002 | US-GRP-009, US-GRP-010 | BR-CONS-006 | US-GRP-012 (`main`), US-GRP-016 (equipo) |
| BR-VAL-003 | US-GRP-014 | BR-CONS-007 | US-GRP-013, US-GRP-016 |
| BR-WF-001 | US-GRP-007, US-GRP-009 | BR-TIME-001 | US-GRP-007, US-GRP-013 |
| BR-WF-002 | US-GRP-014, US-GRP-015 | BR-EDGE-001 | US-GRP-003 |
| BR-AUTH-001 | US-GRP-001, US-GRP-006, US-GRP-010 | BR-EDGE-002 | US-GRP-003 |
| BR-AUTH-002 | US-GRP-001, US-GRP-007 | BR-EDGE-003 | US-GRP-008 |
| BR-CONS-001 | US-GRP-001, US-GRP-012 (+ transversal en todas) | BR-EDGE-004 | US-GRP-008 |
| BR-CONS-002 | US-GRP-010 | BR-EDGE-005 | US-GRP-005, US-GRP-006, US-GRP-014 |
| BR-CONS-003 | US-GRP-002, 007, 008, 009, 010 | BR-EDGE-006 | US-GRP-007 |
| BR-CONS-004 | US-GRP-011, US-GRP-009 (confirmar sesión) | BR-EDGE-007 | US-GRP-015, US-GRP-016 (rama base del equipo) |

**Resultado**: 22 de 22 reglas cubiertas. BR-CONS-007 la cubren US-GRP-013 y US-GRP-016, desbloqueadas el 2026-10-04 (ADR-GRP-007 cierra P8). La parte "configuración del equipo" de BR-CONS-006 y BR-EDGE-007 la verifica US-GRP-016. BR-AUTH-002 es un principio de frontera: en el MVP solo lleva sus dos escenarios exigidos (el motor no modifica hooks, configuración de Git ni metadatos de worktrees, en US-GRP-001; la detección funciona sin hooks propios, en US-GRP-007). Su modelo de permiso explícito no tiene escenarios en el MVP. El rechazo a un agente que cambia los repos observados (Q40) está en una historia Must (US-GRP-001) y se repite con un agente registrado en US-GRP-006. **Codex y Cursor** no tienen historia de soporte completo en el MVP (Q32): quedan cubiertos como "otro agente" por US-GRP-009.

---

## Changelog

| Versión | Fecha | Autor | Cambios |
|---------|-------|-------|---------|
| 1.0 | 2026-10-03 | PO (AADD) para Rene Bonilla | Versión inicial (índice en modo Draft, modelo plano; 15 historias, mapa para la flota y cobertura de las 22 reglas) |
| 1.1 | 2026-10-03 | PO (AADD) para Rene Bonilla | Q32 (solo Claude Code con soporte completo): US-GRP-008 se reemplaza por "el trabajo del desarrollador en su editor nunca se atribuye a Claude Code" (mismo id); Codex y Cursor quedan como "otro agente" en US-GRP-009. US-GRP-012 desbloqueada con `main` provisional (Guardrails y P8 como dependencia futura) y nuevo título; US-GRP-015 sin dependencia de 012. Se retira la marca de supuesto sobre la verificación (decisión de Rene Bonilla). Dependencias, olas y matriz ajustadas. Expansión de las 15 historias en formato compacto para la flota (`expanded: 15`) |
| 1.2 | 2026-10-03 | PO (AADD) para Rene Bonilla | Artifact Judge (FAIL) y decisiones Q33-Q36. Q33: US-GRP-010 reescrita (corregir reemplaza, una sola sesión, no compartido) y US-GRP-011 reescrita (registrar otro agente añade sesión y vuelve compartido). Q34/Q35: US-GRP-008 fusiona el editor del desarrollador y el agente sin registrar en un solo esquema "sin atribuir" y elimina "no identificada". Q36: nueva US-GRP-016 (rama base del equipo), bloqueada por F-001-04 y P8. US-GRP-001 deja el ahead/behind a US-GRP-012. US-GRP-013 bloqueada por P8; el umbral por defecto queda en US-GRP-007. Dependencias nuevas (004 → 007, 009; 006 → 005, 009; 009 → 007; 014 → 015) y US-GRP-005 sin depender de "Sin repos". Condiciones observables en lugar de "la máquina se reinicia/apaga" (US-GRP-004, 005) y "Git se desinstala" (US-GRP-014). US-GRP-002 enlaza la frescura a NFR-04. Mapa de olas y matriz recalculados (`total_artifacts: 16`, `blocked` en el frontmatter) |
| 1.3 | 2026-10-03 | PO (AADD) para Rene Bonilla | Segunda pasada del Artifact Judge (RESERVAS) y decisiones Q37-Q42. US-GRP-010: la corrección alcanza la sesión desde su inicio (Q37), rechazo sin atribución detectada (Q38), rechazo cuando corrige un agente y persistencia tras reiniciar el motor (6 escenarios); depende de 004. US-GRP-009: el primer escenario parte de un worktree sin ninguna sesión y nuevo escenario "registrar un agente ya detectado confirma su sesión" (Q39). US-GRP-011: persistencia del worktree compartido tras reiniciar y referencia a Q39; depende de 004. US-GRP-001 (Must) añade "un agente no puede añadir ni retirar repos" (Q40); US-GRP-006 lo conserva con un agente registrado. US-GRP-008: condición preparable ("un proceso distinto de la sesión de Claude Code") en lugar de "el motor no tiene evidencia"; en serie tras 009. US-GRP-014: el último escenario añade un agente registrado y depende de 005 y 009. US-GRP-012 cita Q42. US-GRP-016 depende de 013. Olas recalculadas (ola 2 solo 009; 008 pasa a la ola 3; 010 y 011 a la 4; 014 a la 5) y matriz de cobertura actualizada |
| 1.4 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisiones heredadas de Guardrails Q-GRD-20 y Q-GRD-21, posteriores a la aprobación del requerimiento. US-GRP-016: la rama base se lee de la copia conocida de la rama principal y el ahead/behind se calcula contra la rama base confirmada; el escenario de cambio pasa a "pendiente de confirmar" hasta la confirmación del desarrollador; nuevos escenarios de cambio que no está en la copia conocida y de falta de confirmación inicial ("no confirmada"); la máquina nueva confirma la rama base al añadir el repo (5 escenarios). Sin historias nuevas |
| 1.5 | 2026-10-04 | PO (AADD) para Rene Bonilla | Decisión heredada de Guardrails Q-GRD-23: US-GRP-016 deja de confirmar la rama base al añadir el repo; en una máquina nueva la rama base del equipo aplica desde la primera consulta, marcada como "no confirmada" hasta que el desarrollador la confirme al proteger el repo o de forma explícita. Sin historias nuevas |
| 1.6 | 2026-10-04 | PO (AADD) para Rene Bonilla | Artifact Judge (FAIL): US-GRP-016 sigue siendo independiente de Guardrails. Sus escenarios parten de "la rama base confirmada …" como precondición, sin la acción de confirmar; "pendiente de confirmar" y "no confirmada" son lo que muestra el motor. Dependencias sin US-GRD-001 ni US-GRD-014; la coherencia se comprueba en la prueba de integración posterior del índice de Guardrails |
| 1.7 | 2026-10-04 | Agente de documentación para Rene Bonilla | Aceptación de ADR-GRP-005 a 013 (Rene Bonilla, 2026-10-04). ADR-GRP-007 cierra P8: US-GRP-013 se desbloquea (pasa a la ola 2, tras 007). US-GRP-016 se desbloquea por decisión del coordinador (2026-10-04, pendiente de confirmar por Rene): Q36 pedía que existieran Guardrails como dueño de la configuración del equipo y el ADR de formato, y los dos existen como ADRs aceptados (ADR-GRD-004, ADR-GRP-007); queda en la ola 3 con dependencia de US-GRP-013 y TS-GRD-001. Sin historias bloqueadas |
