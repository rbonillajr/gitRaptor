---
id: ADR-GRD-005
title: Estado de protección y detección de pérdida
type: adr
status: proposed
date: 2026-10-04
created: 2026-10-04
updated: 2026-10-04
deciders: [Rene Bonilla]
domain: GRP
feature: guardrails
related: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-007, CTX-GRD-001, BR-GRD-001]
tags: [guardrails, estado-proteccion, br-wf-002, deteccion-perdida, integridad, observador, q-grd-15, allowlist-mcp, instalacion-huerfana]
---

# ADR-GRD-005 — Estado de protección y detección de pérdida

## Contexto

BR-WF-002 define cuatro estados por repo: **Sin protección**, **Solo MCP**, **Solo hooks** y **Completa**. El estado depende solo de las capas activas. Las transiciones van al registro y se avisa cuando los hooks dejan de estar activos **sin** que el desarrollador los haya desinstalado desde Guardrails. Q-GRD-15 añade un caso: un repo retirado de la observación sigue protegido por sus hooks, con actor "sin atribuir", y se avisa. US-GRD-004 pide además que el estado muestre el mínimo seguro y la lista de operaciones no impedibles.

La capa de hooks se puede perder por varias causas ajenas a Guardrails:

- Otro gestor reescribe `core.hooksPath` o escribe en la carpeta de Guardrails.
- Alguien edita los dispatchers.
- El binario desaparece o se sustituye.
- El repo se mueve de carpeta.
- Aparece un `core.hooksPath` por worktree.
- Se borra el perfil.

La revisión pide además cuatro cosas:

- La referencia de integridad no puede ser el manifiesto del repo, que el agente puede escribir (H-04).
- Un perfil borrado no puede hacer mentir al estado (J5).
- El encadenado imposible tiene que quedar persistido (J6).
- El modo degradado tiene que ser visible (H-03).

D6 añade los cambios pendientes de rama base y de suelo.

## Decisión

**El estado se deriva, no se guarda. Sale de dos capas comprobadas: hooks activos, verificados en cada worktree contra la referencia de integridad del diario del perfil, y repo en la allowlist del MCP (interfaz con F-001-05). La pérdida se detecta con el observador del motor en los repos observados y con una comprobación periódica en los repos protegidos no observados. Cada transición va al registro, y solo se avisa, con rebote, si la pérdida no la hizo Guardrails.**

### 1. Capa de hooks activa: comprobación

La capa está **activa** solo si se cumplen todas estas condiciones:

| # | Condición | Causa si falla |
|---|---|---|
| H1 | Hay una instalación `confirmada` en el diario del perfil (ADR-GRD-001 § 4) | `no-instalada` o, si hay evidencia en el repo sin entrada en el diario, `instalacion-huerfana` (abajo) |
| H2 | El valor **efectivo** de `core.hooksPath` en **cada** worktree es la ruta absoluta de `<git-common-dir>/gitraptor/hooks` | `hookspath-cambiado` (con el worktree) o `repo-movido` |
| H3 | El hash de cada dispatcher coincide con **la referencia de integridad del diario** (no la del manifiesto), y el `dev/inode` de la carpeta es el guardado | `dispatcher-alterado` o `carpeta-ausente` |
| H4 | La ruta estable del binario resuelve a un destino con firma válida o con la huella fijada (ADR-GRD-001 § 8) | `binario-ausente` o `binario-no-valido` |

- **Instalación huérfana** (J5): el perfil se borró, pero el repo conserva la clave apuntando a `gitraptor/hooks` y un manifiesto.
  - **Qué hace el estado**: se basa en el manifiesto como **evidencia de respaldo, no autoritativa**. Muestra `instalacion-huerfana` con aviso, nunca "Sin protección" a secas.
  - **Qué hacen los hooks** (Judge ronda 2): los dispatchers siguen ejecutándose y llegan al daemon real por la ruta del canal, que es una constante (ADR-GRD-001 § 2). Pero el perfil recreado tiene **otro id de instancia**, así que el cliente aplica el **modo degradado** con `instance-mismatch` (ADR-GRD-003 § 4). Es más estricto que el normal, y nunca bloquea los commits.
  - **Cómo detecta el daemon la huérfana**: compara el id de instancia escrito en los dispatchers con el suyo.
  - **Qué se ofrece**, siempre por comando reservado (ADR-GRD-007):
    - **Adoptar**: el daemon regenera en memoria los dispatchers esperados con las constantes **antiguas** (las del manifiesto y el id de instancia de los dispatchers).
      - **Si coinciden** byte a byte con los del disco: los regenera con las constantes de la instancia actual, con la transacción de ADR-GRD-001 § 4 y como la misma instalación, y vuelve a fijar la referencia de integridad en el diario. Desde ese momento los hooks dejan el modo degradado.
      - **Si no coinciden**: solo se ofrece retirar.
    - **Retirar**: desinstalación con los valores del manifiesto, mostrando antes al humano lo que se va a restaurar.
  - **Reinstalar** sobre una instalación huérfana pasa por adoptar o retirar; no aborta en silencio.
- **Diagnósticos que no cambian el estado** pero se muestran:
  - `hook-previo-no-encadenado`.
  - `encadenado-imposible` (J6): guardado con el último intento de instalación, su fecha y su causa, hasta un intento nuevo o una instalación con éxito (Q-GRD-4, BR-EDGE-002).
  - `repo-no-observado` (Q-GRD-15).
  - `configuracion-ilegible` (ADR-GRD-004).
  - `daemon-unreachable`, `instance-mismatch` o `channel-not-authentic` (H-03): ventana degradada o de deny, con su inicio y su fin.
  - `base-change-pending` y `floor-relax-pending` (D6, ADR-GRD-004 § 3 y § 4).

### 2. Capa MCP (interfaz con F-001-05)

- **Activa** si el repo está en la allowlist del MCP. Guardrails la **consulta** y nunca la escribe.
- **Mientras F-001-05 no exista**, la capa es "no en allowlist", así que los únicos estados posibles son Sin protección y Solo hooks.

### 3. Estado expuesto (canal, `crates/api`)

| Campo | Contenido |
|---|---|
| `state` | `unprotected` \| `mcp-only` \| `hooks-only` \| `full` (BR-WF-002) |
| `hooks` | `active` \| `inactive` \| `not-installed` \| `orphaned`, con `cause` y el worktree afectado |
| `minimumSet` | `active` \| `disabled-by-team`, con las reglas, la rama o ramas base protegidas y si hay un cambio pendiente |
| `notPreventable[]` | La lista publicada de ADR-GRD-002 § 3, con los saltos declarados |
| `diagnostics[]` | Los del § 1, como códigos |
| `permission` | `never-asked` \| `granted` \| `denied` (BR-AUTH-002) |

El texto lo pone el cliente (NFR-10). Lo presentan la CLI y el Cockpit.

### 4. Detección de pérdida

| Repo | Disparador | Latencia objetivo |
|---|---|---|
| Observado y protegido | El observador del motor notifica cambios en `<git-common-dir>/config`, en el `config.worktree` de cada worktree y en `<git-common-dir>/gitraptor/`; también el alta de un worktree y los cambios de las refs de la copia de la rama principal (D6) | ⚠️ **ASSUMPTION**: ≤ 5 s |
| Protegido y no observado (Q-GRD-15) | Comprobación periódica del § 1 | ⚠️ **ASSUMPTION**: cada 60 s, con jitter |
| Cualquiera | Al arrancar el daemon; al consultar el estado; al volver de una ventana degradada | Inmediata |

- **Dependencia**: ADR-GRP-010 tiene que vigilar esas rutas en los repos protegidos (tabla de enmiendas).

### 5. Transiciones, registro y aviso

- **Qué se anota**: cada cambio de `state`, de `hooks` o de los diagnósticos de pérdida va al registro como `protection-state` con su causa (ADR-GRD-006). La instalación, la desinstalación, la adopción y el refresco de integridad (ADR-GRD-001 § 8) van además a la auditoría permanente.
- **¿Lo hizo Guardrails?**:
  - **Sí**: antes de escribir, el daemon anota en el diario una **transición esperada**. Una pérdida que coincide con ella no genera aviso (US-GRD-004, escenario 2).
  - **No**: aviso `protection-lost` con la causa, por el stream de eventos. Queda pendiente si no hay ningún cliente abierto.
- **Rebote de avisos** (L-01): por repo y causa, un aviso como máximo cada ⚠️ **ASSUMPTION** 10 minutos mientras la causa persista. La primera transición siempre avisa y siempre se registra.
- **Repo retirado de la observación con hooks activos**: aviso `protected-but-unobserved`.
- **No hay reparación automática**: volver a activar una capa es una instalación nueva con permiso. La adopción de una huérfana y el refresco por actualización del binario son la misma instalación (ADR-GRD-001 § 8).

## Alternativas consideradas

| Alternativa | Por qué no |
|---|---|
| Guardar el estado y actualizarlo solo al instalar o desinstalar | Mentiría tras una pérdida externa |
| Integridad contra el manifiesto del repo | El agente lo edita junto con el dispatcher (H-04) |
| Perfil borrado = "Sin protección" | Mentiría: los hooks siguen activos (J5) |
| Detectar la pérdida desde los propios hooks | Un hook que no se ejecuta no avisa |
| Reparar automáticamente | Cambia lo instalado sin permiso y entra en guerra con otros gestores |
| Avisar en cada comprobación | Satura al humano cuando la causa persiste (L-01) |

## Consecuencias

- ✅ El estado no miente, tampoco tras borrar el perfil, y cada causa tiene nombre.
- ✅ La integridad se ancla en lo que solo escribe el daemon.
- ✅ El modo degradado y los cambios pendientes del suelo y de la rama base son visibles.
- ⚠️ **Dependencia**: ADR-GRP-010 tiene que vigilar las rutas del § 4.
- ⚠️ Un salto de un solo comando no cambia el estado: lo declara ADR-GRD-002 § 2.
- ⚠️ Un proceso del mismo usuario puede borrar el perfil **y** la carpeta a la vez. Entonces el repo queda sin protección y la única pista es la clave rota. Se detecta como `carpeta-ausente` con la clave apuntando a Guardrails.

## Validación

1. **Estados**: Sin protección y Solo hooks (MVP); Solo MCP y Completa con una allowlist simulada (US-GRD-016).
2. **Pérdida externa**: husky reescribe la clave, se edita un dispatcher, se borra la carpeta, se sustituye el binario por otro sin firma, se mueve el repo y aparece `core.hooksPath` en un `config.worktree`. En cada caso, la causa correcta, en el plazo del § 4, con aviso y entrada en el registro (US-GRD-004, escenario 1).
3. **Integridad** (H-04): editar a la vez un dispatcher y el manifiesto para que cuadren → `dispatcher-alterado`.
4. **Huérfana** (J5; Judge ronda 2): borrar el perfil con la protección instalada da `instalacion-huerfana` con aviso. Mientras tanto, los hooks llegan al daemon real en modo degradado con `instance-mismatch`: un commit pasa y un force-push se deniega. Adoptar con los dispatchers intactos regenera las constantes y deja el estado en `active`, sin modo degradado. Adoptar con un dispatcher alterado solo ofrece retirar. Retirar deja la huella de antes de instalar.
5. **Encadenado imposible** (J6): un intento fallido deja `encadenado-imposible` con su causa en el estado, también tras reiniciar el daemon.
6. **Desinstalación propia**: pasa a Sin protección sin aviso (US-GRD-004, escenario 2).
7. **Repo retirado**: con los hooks activos, el force-push sigue denegado con "sin atribuir", hay aviso y la comprobación periódica detecta una pérdida posterior (US-GRD-004, escenario 3).
8. **Modo degradado** (H-03): con el daemon imposible de arrancar, aparece `daemon-unreachable` con su ventana al volver.
9. **Rebote** (L-01): una causa persistente durante una hora da un aviso cada 10 minutos y una sola transición en el registro.
10. **Mínimo y lista**: sin configuración, `minimumSet = active` y `notPreventable` coincide con la regresión de ADR-GRD-002 (US-GRD-004, escenarios 4 y 5).
11. **Sin escrituras** en el repo por la comprobación (INF-GRP-001).

## Referencias

- **Reglas**: BR-WF-002, BR-EDGE-001, BR-EDGE-002, BR-EDGE-003, BR-AUTH-002; Q-GRD-4, Q-GRD-15; D6 (Rene Bonilla, 2026-10-04).
- **Historias**: US-GRD-001, US-GRD-003, US-GRD-004, US-GRD-016; US-GRP-006.
- **ADRs de otros frentes**: ADR-GRP-005, ADR-GRP-006, ADR-GRP-010, ADR-GRP-013 (`docs/arch-motor-local`).
- **Seguridad**: SEC-GRD-02, SEC-GRD-08, SEC-GRD-16.

## Revisión de seguridad (2026-10-04)

| Hallazgo | Cómo se cubre |
|---|---|
| H-04 · Ancla de integridad escribible | § 1 H3 y H4: la integridad se comprueba contra el diario del perfil, y el binario por firma o huella; Validación 3 |
| J5 · Perfil borrado | § 1: `instalacion-huerfana` con el manifiesto como evidencia de respaldo; adoptar o retirar por comando reservado; Validación 4 |
| J6 · Encadenado imposible | § 1: diagnóstico persistido con el intento; Validación 5 |
| H-03 · Modo degradado invisible | § 1 y § 4: `daemon-unreachable` y `channel-not-authentic` con su ventana; Validación 8 |
| L-01 · Saturación de avisos | § 5: rebote por repo y causa; Validación 9 |
| D6 · Cambios pendientes | § 1 y § 3: `base-change-pending` y `floor-relax-pending` visibles |
| J13 · Referencias rotas en el frontmatter | `related` solo con IDs existentes |
| Judge ronda 2, hallazgos 1 y 3 · Huérfana con constantes e instancia | § 1: los hooks llegan al daemon real y aplican el modo degradado con `instance-mismatch` hasta la adopción, que regenera las constantes; Validación 4 |
