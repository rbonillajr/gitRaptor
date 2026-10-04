---
id: ADR-GRD-007
title: Acciones reservadas al humano y excepción consciente
type: adr
status: accepted
accepted: 2026-10-04
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-001, ADR-GRD-003, ADR-GRD-004, ADR-GRD-005, ADR-GRD-006, ADR-CKP-002, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, comandos-reservados, excepcion-consciente, token-un-solo-uso, br-auth-001, br-auth-002, r-grd-3, prompt-injection, ascendencia, d5, factor-fuera-de-banda]
---

# ADR-GRD-007 — Acciones reservadas al humano y excepción consciente

> **Estado**: aceptado por Rene Bonilla el 2026-10-04.

## Contexto

BR-AUTH-001 reserva al humano estas acciones: instalar y desinstalar la protección (con permiso explícito, BR-AUTH-002), usar una excepción consciente (Q-GRD-1), relajar la configuración (US-GRD-013) y decidir en la cola (US-GRD-015). Un agente puede usar la terminal, así que esas acciones exigen una confirmación que un agente no pueda dar desde su canal (riesgo R-GRD-3, crítico).

ADR-GRP-005 § 6 (aceptado el 2026-10-04) define los **comandos reservados**, autorizados solo en el daemon:

1. Identificador no reutilizable del llamante.
2. Ascendencia sin un proceso de agente (ADR-GRP-012).
3. Terminal de control y líder de sesión que no desciende de un agente.
4. Confirmación interactiva solo como UX.
5. Exclusión del MCP.
6. Auditoría append-only.

**Decisiones de Rene Bonilla (2026-10-04)**:

- **Decisión 3**: la excepción consciente se ejecuta con `raptor guard exec -- git <args…>` (nombre provisional), que es un comando reservado y emite un **token de un solo uso**. El riesgo residual se acepta como en ADR-GRP-005.
- **D5, "Riesgo MVP + gate"** (H-01). La revisión de seguridad muestra que los controles 1 a 3 **no detectan** varios vectores realistas. D5 responde así:
  - US-GRD-003 (desinstalar) y US-GRD-006 (excepción) mantienen el mecanismo de ADR-GRP-005 § 6, reforzado con:
    - **anuncio** en todos los clientes;
    - una **ventana cancelable** antes de aplicar (⚠️ **ASSUMPTION**: 10 s; la excepción también la respeta, antes de emitir el token);
    - **auditoría con la cadena completa de ascendencia**: ruta del ejecutable e identificador de cada proceso, terminal de control y líder de sesión.
  - La **aceptación del riesgo se registra por acción**.
  - **Requisito duro antes de US-GRD-013 y US-GRD-015**: un **factor fuera de banda del SO** para las acciones que relajan (LocalAuthentication en macOS, Windows Hello / UserConsentVerifier, polkit `auth_self` en Linux). Si no está disponible → **fail-closed**. Mecanismo: [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) (aceptado el 2026-10-04).
  - **Instalar** endurece y mantiene ADR-GRP-005 sin ventana.

## Decisión

**Las acciones reservadas de Guardrails amplían la lista de comandos reservados de ADR-GRP-005 § 6 y usan el mismo mecanismo, decidido en el daemon. Las acciones que relajan añaden anuncio, ventana cancelable y auditoría completa en el MVP, y un factor fuera de banda obligatorio antes de US-GRD-013 y US-GRD-015. La excepción consciente es un token de un solo uso ligado al `git` hijo directo del `raptor` que lo pidió y a la transición exacta.**

### 1. Lista ampliada de comandos reservados

| Comando (nombres provisionales) | Historia | ¿Relaja? | Controles |
|---|---|---|---|
| Instalar la protección (`raptor guard install`) | US-GRD-001, US-GRD-002 | No | ADR-GRP-005 § 6, puntos 1 a 3; confirmación UX con qué, dónde, por qué y cómo se revierte, y los hooks previos (BR-AUTH-002). Sin ventana (D5) |
| Confirmar la rama base y el suelo **iniciales** (**D9**, Rene Bonilla, 2026-10-04; ADR-GRD-004 § 3.5) | US-GRD-001 (al instalar), US-GRD-014 (comando explícito) | **US-GRD-001: no** (confirma `main` sin leer el suelo; con configuración del equipo deja `base-unconfirmed`). **US-GRD-014: sí**, si el suelo trae relajaciones (p. ej. desactiva el mínimo) | ADR-GRP-005 § 6. US-GRD-001, **sin ventana**. US-GRD-014, con **D5** (anuncio, ventana cancelable y auditoría completa) cuando el suelo relaja. **Nunca** al añadir el repo a la observación (motor-local) |
| Registrar la denegación del permiso | US-GRD-001 | No | ADR-GRP-005 § 6 |
| Adoptar una instalación huérfana (ADR-GRD-005 § 1) | US-GRD-003 | No | ADR-GRP-005 § 6. Adoptar no confirma la rama base ni el suelo; quedan `base-unconfirmed` hasta la confirmación explícita (US-GRD-014; ADR-GRD-004 § 3.5) |
| Desinstalar la protección (`raptor guard uninstall`) | US-GRD-003 | **Sí** | ADR-GRP-005 § 6 + **D5**: anuncio, ventana cancelable, auditoría completa y aceptación de riesgo por acción |
| Retirar una instalación huérfana | US-GRD-003 | **Sí** | Igual que desinstalar |
| Excepción consciente (`raptor guard exec -- git …`) | US-GRD-006 | **Sí** | ADR-GRP-005 § 6 + **D5** (la ventana va antes de emitir el token) + el token del § 3 |
| Confirmar un cambio de rama base o una relajación del suelo (ADR-GRD-004 § 3 y § 4) | US-GRD-014, US-GRD-007 (no bloqueadas por este ADR; P8 quedó cerrada por ADR-GRP-007, aceptado el 2026-10-04) | **Sí** | **D8 (Rene Bonilla, 2026-10-04)**: el mecanismo MVP de D5, igual que desinstalar (anuncio, ventana cancelable, auditoría con la ascendencia completa y aceptación de riesgo por acción). Cuando exista el factor fuera de banda, se aplicará también aquí, pero **no bloquea** US-GRD-007 ni US-GRD-014 |
| Relajar la configuración con el comando de edición | US-GRD-013 | **Sí** | ADR-GRP-005 § 6 + **factor de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) obligatorio**; sin él, fail-closed (D5). Endurecer no es reservado |
| Aprobar una petición de la cola | US-GRD-015 | **Sí** | ADR-GRP-005 § 6 + **factor de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) obligatorio** (D5) |
| Rechazar una petición de la cola | US-GRD-015 (bloqueada) | No | ADR-GRP-005 § 6 |

- **Contenido de la confirmación de D8** (cambio de suelo o de rama base): la confirmación y su anuncio en todos los clientes muestran:
  - el **diff de lo que se relaja** entre el suelo confirmado y el nuevo, regla a regla (p. ej. "mínimo seguro: activo → desactivado", "force-push: denegar → permitir", rama base `main` → `develop`);
  - la **ref y el commit de origen** del suelo nuevo (p. ej. `refs/remotes/origin/main` en `<commit>`).

  Sin ese diff no se puede confirmar. Todo ese texto es no confiable y va saneado (SEC-GRD-06). Así el humano ve qué acepta y desde dónde llega, y una ref forjada en local queda a la vista antes de confirmarla.
- **Fuera del MCP**: `raptor-mcp` no expone ninguno, y el canal rechaza estos métodos desde la conexión del MCP (BR-AUTH-004; SEC-14 de motor-local).
- **Permiso explícito** (BR-AUTH-002):
  - Se ofrece solo en un repo observado (Q-GRD-15).
  - **Sin respuesta** no se instala nada.
  - **Tras una denegación** se guarda `permission = denied` y no se vuelve a ofrecer hasta que el desarrollador lo active a mano.
  - **Alcance**: el permiso es por repo.
- **Anuncio y ventana** (D5):
  - **Anuncio**: el daemon publica `reserved-action-pending` en el stream de **todos** los clientes, con la acción, el repo y la cadena de ascendencia resumida.
  - **Ventana**: la acción solo se aplica al cerrarse la ventana. Cualquier cliente del usuario puede cancelarla, y cancelar no es reservado porque solo mantiene la protección.
  - **Registro**: la cancelación y la aplicación quedan en la auditoría.
- **Interfaz para el Cockpit** (Q-GRD-1): una confirmación del humano en una superficie de GitRaptor que choca con una regla cuenta como excepción. Usa el mismo comando con el token del § 3. Se implementa con F-001-02. (Enmienda 2026-10-04, Cockpit: desde el Cockpit no se emite token; ver la sección final.)
- **Toda excepción pasa por D5** (**D10**, Rene Bonilla, 2026-10-04): la de `raptor guard exec` y también la aprobación explícita en el Cockpit llevan el mismo anuncio, la misma ventana cancelable antes de emitir el token y la misma auditoría con la cadena completa de ascendencia y la aceptación del riesgo por acción. No hay ninguna vía de excepción sin ventana.

### 2. Análisis de vectores y aceptación del riesgo por acción (H-01)

| Vector | ¿Lo detectan los controles 1 a 3? | Qué lo limita en el MVP | Qué lo cierra |
|---|---|---|---|
| El agente desciende directamente del proceso que pide la acción | **Sí** (ascendencia) | — | ADR-GRP-005 § 6 |
| Pty bajo el agente (`script`) | **Sí** (líder de sesión) | — | ADR-GRP-005 § 6 |
| `setsid` + `script`, doble fork, `launchctl submit`, `systemd-run --user` (desacople del árbol) | **No** | Anuncio, ventana, auditoría completa | Factor fuera de banda |
| `tmux send-keys` o escritura en la pty de la terminal del humano | **No** (el proceso es hijo de la shell del humano) | Anuncio, ventana, auditoría (el líder de sesión y la terminal quedan registrados) | Factor fuera de banda |
| `osascript` / AppleScript, o automatización de la terminal en Windows (`SendKeys`, UI Automation) | **No** | Anuncio, ventana, auditoría | Factor fuera de banda (el prompt del SO no se puede automatizar sin permisos de accesibilidad que el usuario concede aparte) |
| **Código plantado que el humano ejecuta sin saberlo** | **No** (corre dentro del árbol del humano) | Bajo `raptor guard exec` **se neutralizan** con `-c`: `core.fsmonitor`, `core.pager`, `core.editor`, `sequence.editor`, el diff externo y `textconv`. **No se neutralizan**, porque la operación los necesita o porque son del usuario: `credential.helper` y `core.sshCommand` (autenticación del push), los filtros `clean`/`smudge`/`process` (contenido de un rebase), `gpg.program` cuando la firma está activa, y los **hooks previos encadenados** (NFR-07). Fuera de `guard exec` no se neutraliza nada: el humano usa su Git. Lo no neutralizado es un **riesgo aceptado por D5**, limitado por el anuncio, la ventana y la auditoría. Código plantado que, por ejemplo, lanza `raptor guard uninstall` desde el árbol del humano pasa los controles 1 a 3, pero se anuncia y se puede cancelar. Un `git` lanzado por ese código **no hereda el token**, porque su padre directo no es el `raptor` solicitante | Factor fuera de banda |

- **Aceptación por acción**: cada desinstalación y cada excepción guardan en la auditoría:
  - la referencia a la versión de esta tabla;
  - los vectores **no cubiertos** en el momento;
  - la cadena completa de ascendencia;
  - la terminal de control;
  - el líder de sesión.

  La auditoría registra el riesgo aceptado **en cada acción**, no solo en este documento.
- **Por qué es aceptable en el MVP** para desinstalar, para la excepción y para las confirmaciones de D6 (D8):
  - Todos dejan un rastro visible y cancelable.
  - Afectan a una operación o a un repo.
  - Las acciones que relajan de forma persistente y a discreción del humano (editar la configuración y aprobar en la cola) no salen sin el factor fuera de banda.
  - La confirmación del suelo solo acepta lo que ya está commiteado en la rama principal.
- **Windows** (M-07). Criterio de humano frente a agente:
  - **Ascendencia**: por el padre con la comprobación de la hora de creación, a través del handle del proceso.
  - **Terminal**: el cliente está adjunto a una consola interactiva y la cadena de procesos dueña de esa consola (la shell bajo conhost, OpenConsole o Windows Terminal) no desciende de un agente.
  - ⚠️ **ASSUMPTION**: viabilidad a confirmar en SPIKE-GRD-001 y en el SPIKE de detección de motor-local.

### 3. Excepción consciente: token de un solo uso (decisión 3; H-02; J9; L-04)

1. **Normalización previa** (J9): la CLI normaliza el argv con el mismo traductor que los hooks.
   - **Qué fija**: la operación, las refs objetivo y el worktree (su cwd).
   - **Qué resuelve**: `-C`, push sin refspec según `push.default`, `--all`, `--mirror`, las refs simbólicas y los alias de ADR-GRD-002 § 4.
   - **Si el argv no se puede normalizar** a transiciones concretas (p. ej. `--mirror` con borrados implícitos, o un refspec con comodines ambiguos): se rechaza **antes** de emitir el token, con el motivo. SPIKE-GRD-001 cubre los casos.
2. **Autorización** en el daemon: ADR-GRP-005 § 6 sobre el proceso `raptor`. Si falla: rechazo y anotación en la auditoría y como `exception-rejected` (US-GRD-006, escenario 4).
3. **Anuncio y ventana** (D5, D10). Al cerrarse sin cancelación, pasa a la emisión. Si el humano la cancela dentro de la ventana, no se emite token y se registra con `kind = exception-cancelled` (ADR-GRD-006 § 1), que no cuenta en el KPI.
4. **Emisión**: un token aleatorio de ≥ 128 bits (CSPRNG del SO). El daemon guarda **solo su hash**, ligado a:
   - la **transición exacta** `(ref, viejo, nuevo)` cuando se conoce (push, borrado), tomando como valor viejo esperado la copia de seguimiento conocida;
   - si la transición no se conoce de antemano (rebase), la ref más la base;
   - el repo y el worktree;
   - la identidad del `raptor` solicitante;
   - un TTL. ⚠️ **ASSUMPTION**: 60 s.
5. **Ejecución**: `raptor` lanza `git` por la ruta validada (ADR-GRP-009 § 4), con argv fijo y sin shell.
   - **Ejecutables neutralizados**: se añaden `-c` para `core.fsmonitor`, `core.pager`, `core.editor`, `sequence.editor`, el diff externo y `textconv` (H-02). Por eso `rebase -i` no está disponible bajo la excepción.
   - **Token**: va en el entorno **solo del hijo**.
   - **Registro del hijo**: antes de esperar al hijo, `raptor` **registra en el daemon la identidad del hijo `git`**: `(pid, hora de inicio)` o pidfd, el audit token en macOS o el handle en Windows.
6. **Presentación**: el hook gobernado envía el token con la evaluación. El daemon lo acepta solo si se cumplen todas estas condiciones:
   - el hash coincide (comparación en tiempo constante);
   - el TTL no ha vencido;
   - el **`git` antecesor más cercano** del hook es el hijo registrado, y su **padre directo** es el `raptor` solicitante, con la identidad verificada en cada salto;
   - la transición evaluada es exactamente la ligada (o, en un rebase, la misma ref y la misma base).

   Si el registro del hijo aún no ha llegado, el daemon lo espera un tiempo acotado.
7. **Consumo y vida** (L-04):
   - **Consumo**: la primera presentación válida lo consume de forma atómica y lo liga a ese `git`.
   - **Vida**: el TTL rige **solo hasta la primera presentación**. Después, la vida del token es la del `git` ligado.
   - **Reutilización**: las evaluaciones posteriores **de la misma transición** desde ese `git` reutilizan la decisión y solo deduplican el registro. Una transición distinta **no** queda cubierta (ADR-GRD-003 § 6).
8. **Resultado**:
   - La decisión vuelve con `exception = applied` y `appliedEffect = allow`. Se anota como `exception` y en la auditoría con la aceptación de riesgo del § 2.
   - Una segunda operación se deniega como antes.
9. **Higiene**:
   - El encadenado quita la variable del token del entorno del hook previo (ADR-GRD-001 § 2).
   - El token nunca aparece en logs, registros ni salidas.
   - Los mensajes al agente no mencionan la excepción (ADR-GRD-003 § 3).
   - En modo degradado no hay excepción.

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Variable o bandera "soy humano" sin token | Cualquier agente la pondría |
| Solo la confirmación interactiva | Un agente con pty la supera (ADR-GRP-005, PQ-6) |
| Excepción "por tiempo" | No vale para una sola operación (Q-GRD-1, S-GRD-8) |
| Token ligado a "cualquier antecesor" o solo a la operación | Un `git` del agente bajo un proceso legítimo, u otra transición del mismo comando, lo aprovecharía (H-02) |
| `git --no-verify` como "excepción" | Sin registro ni regla saltada, y no salta `reference-transaction` |
| Factor fuera de banda ya en el MVP para todas las acciones | Más fuerte, pero depende de cada SO y retrasa el esqueleto andante. D5 lo exige antes de las acciones que relajan de forma persistente |
| Solo los controles de ADR-GRP-005 sin reforzar | No detectan `tmux send-keys`, `osascript` ni el código plantado (H-01) |

## Consecuencias

- ✅ Un solo mecanismo para todas las acciones reservadas, con el riesgo aceptado por acción y auditable.
- ✅ Un token robado no sirve: falta el padre directo y la transición exacta.
- ✅ Las acciones que relajan de forma persistente no saldrán sin el factor fuera de banda (gate duro de D5), **salvo las confirmaciones de D8** (cambio de suelo o de rama base), que usan el mecanismo MVP con el diff y el origen a la vista.
- ⚠️ **Riesgo residual de D8**: un agente puede forjar `refs/remotes` con un suelo laxo y después conseguir la confirmación con un vector no detectable (`tmux send-keys`, `osascript`, código plantado). Lo limitan el diff y el origen visibles, el anuncio, la ventana y la auditoría. Se cierra cuando el factor fuera de banda se aplique también a D8.
- ⚠️ **Riesgo residual del MVP** (D5, decisión 3): para desinstalar y para la excepción, los vectores de la tabla del § 2 que dicen "No" siguen abiertos, limitados por el anuncio, la ventana y la auditoría.
- ⚠️ **La ventana de 10 s añade espera** a desinstalar y a la excepción. Es un coste aceptado por D5.
- ⚠️ **`rebase -i` no está disponible bajo la excepción** por la neutralización de `sequence.editor`.
- ⚠️ **Dependencias**: la lista de comandos reservados de ADR-GRP-005 § 6 y SEC-03 de motor-local se amplía con los de este ADR: **aplicada (2026-10-04)** (tabla de enmiendas). El factor fuera de banda lo define [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md), aceptado el 2026-10-04.

## Validación

1. **Agente rechazado**: `install`, `uninstall` y `exec` desde un agente simulado (por CLI, JSON-RPC directo y pty) se rechazan y se auditan (US-GRD-006, escenario 4).
2. **MCP**: el catálogo no los ofrece y el canal los rechaza desde la conexión del MCP.
3. **Anuncio y ventana** (D5): un `uninstall` aparece en otro cliente abierto; cancelado dentro de la ventana, no se aplica y la cancelación se audita. Un `exec` cancelado no emite token.
4. **Auditoría completa** (D5): la entrada lleva la cadena de ascendencia con ruta e identificador de cada proceso, la terminal, el líder de sesión y los vectores no cubiertos.
5. **Vectores** (H-01): un `tmux send-keys` en la pty del humano que lanza `raptor guard uninstall` **se anuncia, es cancelable y queda auditado**. Es el comportamiento documentado del riesgo aceptado.
6. **Excepción**: un force-push denegado se ejecuta con `raptor guard exec` y la entrada `exception` nombra la regla (US-GRD-006, escenario 2). Un segundo force-push se deniega (escenario 3).
7. **Ligadura** (H-02): el token copiado a un `git` que no es hijo directo del `raptor` solicitante se rechaza; también el caducado antes de la primera presentación; también una transición distinta (otro `new` u otra ref) del mismo `git`.
8. **Normalización** (J9): `push` sin refspec con `push.default` en `simple`, `current` y `matching`; `--all`; `-C <dir>`; `--mirror` → transiciones concretas o rechazo antes de emitir el token.
9. **Neutralización** (H-02): con `core.pager`, `core.fsmonitor` y `sequence.editor` apuntando a un script canario, el canario no se ejecuta bajo `raptor guard exec`.
10. **Higiene**: el hook previo no ve el token; los logs y el registro no lo contienen.
11. **Permiso**: si se deniega o no hay respuesta, no se instala nada; un permiso no alcanza a otro repo (US-GRD-001, escenarios 5 y 6).
12. **Windows** (M-07): un cliente lanzado desde un proceso de agente simulado, con y sin consola propia, se rechaza.
13. **Confirmación inicial** (D9): añadir un repo a la observación no confirma nada. Instalar (US-GRD-001) en un repo con configuración del equipo no lee el suelo y deja `base-unconfirmed`, sin ventana. La confirmación explícita (US-GRD-014) de un suelo que desactiva el mínimo se anuncia y espera la ventana; cancelada, no confirma nada.
14. **Excepción desde el Cockpit** (D10): una aprobación explícita en el Cockpit se anuncia en los demás clientes, es cancelable en la ventana y queda auditada con la ascendencia completa.

## Referencias

- **Reglas**: BR-AUTH-001, BR-AUTH-002, BR-AUTH-003, BR-AUTH-004, BR-CONS-004; Q-GRD-1, Q-GRD-3, Q-GRD-13, Q-GRD-15; S-GRD-8; R-GRD-2, R-GRD-3.
- **Decisiones**: decisión 3, D5, D6, D7, D8, D9 y D10 de Rene Bonilla (2026-10-04).
- **Historias**: US-GRD-001, US-GRD-002, US-GRD-003, US-GRD-006, US-GRD-007, US-GRD-014; requisito duro para US-GRD-013 y US-GRD-015.
- **ADRs de otros frentes**: ADR-GRP-005 § 6, ADR-GRP-009 § 4, ADR-GRP-012, ADR-GRP-013 (motor-local, en `main`).
- **Seguridad**: SEC-GRD-04, SEC-GRD-05; SEC-03 de motor-local; OWASP LLM01 y LLM06.

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| H-01 · Los controles de ADR-GRP-005 no detectan varios vectores | § 2: análisis de vectores; D5: anuncio, ventana cancelable, auditoría completa, aceptación por acción y factor fuera de banda obligatorio antes de US-GRD-013 y US-GRD-015; Validación 3 a 5 |
| H-02 · Ligadura del token | § 3: `git` más cercano con el `raptor` como padre directo, identidad en cada salto, registro del hijo, transición exacta, reutilizar solo deduplica, `-c` que neutralizan ejecutables; Validación 7 y 9 |
| L-04 · TTL | § 3 paso 7: el TTL rige hasta la primera presentación; después, la vida del `git` ligado |
| J9 · Normalización del argv | § 3 paso 1: casos resueltos o rechazo antes del token; Validación 8; caso en SPIKE-GRD-001 |
| M-07 · Windows | § 2: criterio de humano y agente por handle, hora de creación y consola dueña; Validación 12 |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |
| D8 · Confirmar un cambio de suelo | § 1: el mecanismo MVP de D5; el factor fuera de banda se aplica cuando exista, pero no bloquea US-GRD-007 ni US-GRD-014; el gate duro queda solo en US-GRD-013 y US-GRD-015 |
| Judge ronda 2, hallazgo 4 · Código plantado | § 2: qué neutraliza `guard exec` y qué no (`credential.helper`, `core.sshCommand`, filtros, `gpg.program`, hooks previos); lo restante es un riesgo aceptado por D5 |

## Cambios (2026-10-04, coherencia con motor-local)

- § 1: fila nueva para la confirmación inicial (**D9**): US-GRD-001 al instalar, sin relajar y sin ventana; US-GRD-014 explícita, con D5 si el suelo relaja; nunca al añadir un repo. Validación 13.
- § 1: **D10**: toda excepción, también la aprobación en el Cockpit, pasa por el anuncio, la ventana y la auditoría de D5. Validación 14.
- § 1: adoptar o retirar una instalación huérfana pasa a US-GRD-003.
- Consecuencias: la ampliación de ADR-GRP-005 § 6 y SEC-03 pasa a "aplicada (2026-10-04)".
- § 3: la ventana cancelable nombra el `kind` `exception-cancelled` (ADR-GRD-006 § 1) para la excepción cancelada.
- Corrección tras el Judge: la confirmación inicial de US-GRD-001 no relaja y no tiene ventana; D5 solo en la explícita de US-GRD-014 (§ 1, Validación 13). D9 y D10 en Referencias.
- Judge de la rama del PO: la fila Adoptar dice que adoptar no confirma la rama base ni el suelo (`base-unconfirmed` hasta la confirmación explícita).

## Enmienda (2026-10-04, ADR-GRD-008)

Aplicada desde la tabla de enmiendas de [ADR-GRD-008](./ADR-GRD-008-factor-autenticacion-fuera-de-banda.md). El `status` sigue en `accepted`.

- § 1: las filas de relajar con el comando y aprobar en la cola citan el factor de ADR-GRD-008; ya no figuran como bloqueadas por falta de ADR.
- § 2: la columna "Qué lo cierra" ("Factor fuera de banda") se lee como **ADR-GRD-008, en las acciones que lo adoptan**: hoy US-GRD-013, US-GRD-015 y la confirmación de una relajación personal (Q-GRD-32). Desinstalar, la excepción y las confirmaciones de D8 lo adoptan en modo preferente en una historia posterior (OQ-GRD-008-3). El factor cierra también el riesgo A-2 de ADR-GRP-005 (Enmienda TS-GRP-004, punto 9) en esas acciones.
- Consecuencias: "requiere un ADR propio" pasa a ADR-GRD-008.

## Enmienda (2026-10-04, Cockpit)

Aplicada desde Q-CKP-15 y DEP-CKP-10 de [CTX-CKP-001](../../requirements/features/cockpit/context.md), con [ADR-CKP-002](./ADR-CKP-002-catalogo-operaciones-ejecutor.md) § 4 (proposed). **Decisión del orquestador (2026-10-04), validada por Arquitecto**; el PO valida el alcance después. No cambia la lista de comandos reservados, D5, D10, el análisis de vectores ni el token de `raptor guard exec`. El `status` sigue en `accepted`.

| Cambio | Dónde | Fuente |
|---|---|---|
| La excepción consciente desde el Cockpit **no emite un token en el entorno**: la ligadura es el registro del hijo del ejecutor más la huella del plan, con los mismos controles, D5 y D10 | § 1, "Interfaz para el Cockpit" | Q-CKP-15; ADR-CKP-002 § 4 |

- **Por qué sin token**: en `raptor guard exec`, el token liga la excepción al `git` que lanza un `raptor` distinto del daemon. En el Cockpit, quien emite la decisión y quien lanza `git` son el **mismo daemon** (el ejecutor de ADR-CKP-002). La ligadura de un solo uso a la transición exacta la dan el registro del hijo en el mismo paso del lanzamiento y la huella del plan (ADR-GRD-003, Enmienda (2026-10-04, Cockpit)).
- **Mismos controles**: es un comando reservado sobre el proceso de la TUI (ADR-GRP-005 § 6, puntos 1 a 3). Lleva **D5** (anuncio `reserved-action-pending` en todos los clientes, ventana cancelable y auditoría con la ascendencia completa y la aceptación del riesgo por acción) y **D10**. La ventana va **antes de ejecutar** el plan, en lugar de antes de emitir el token. ⚠️ **ASSUMPTION** heredada: ventana de 10 s (S-CKP-3).
- **Rechazos**: si el solicitante es un agente, la excepción se rechaza y se anota como `exception-rejected`. El MCP nunca la ofrece (§ 1).
- **Registro**: `exception`, `exception-cancelled` o `exception-rejected` con `layer = cockpit`, una sola entrada por plan (ADR-GRD-006, Enmienda (2026-10-04, Cockpit)).
- **Sin cola**: "pedir confirmación" se aplica como denegar sin cola mientras US-GRD-015 y el factor fuera de banda no existan (DEP-CKP-8).
- **Validación añadida**: una excepción del Cockpit cancelada en la ventana deja `exception-cancelled` y no ejecuta nada; aplicada, una sola `exception`; pedida desde un proceso que desciende de un agente, `exception-rejected`; una transición distinta de la registrada se evalúa de nuevo.
