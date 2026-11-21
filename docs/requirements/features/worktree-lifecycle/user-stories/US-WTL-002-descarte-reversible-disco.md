---
id: US-WTL-002
title: "El desarrollador descarta el worktree de un agente terminado, sabe cuánto disco ganó y lo puede restaurar"
type: us
status: ready
priority: high
milestone: M3
blocked_by:
  - US-CKP-017
  - US-WTL-003
created: 2026-11-10
updated: 2026-11-21
feature: worktree-lifecycle
related:
  context:
    - CTX-WTL-001
  adrs:
    - ADR-TMC-001
    - ADR-TMC-007
    - ADR-CKP-002
    - ADR-GRP-012
  stories:
    - US-CKP-017
    - US-WTL-003
    - US-WTL-005
    - US-TMC-002
    - US-CKP-021
    - US-DSK-016
  rules:
    - BR-WTL-006
    - BR-WTL-007
    - BR-WTL-008
    - BR-WTL-011
  specs:
    - DS-US-WTL-002
ado:
  id: null
  url: null
tags:
  - worktree-lifecycle
  - descartar
  - time-machine
  - disco
  - must
---

# US-WTL-002: El desarrollador descarta el worktree de un agente terminado, sabe cuánto disco ganó y lo puede restaurar

## Descripción

**Como** desarrollador orquestador, **quiero** que el descarte de un worktree solo se permita con su agente terminado, me diga el disco que gané de verdad y me deje restaurarlo durante la retención aunque haya hecho otras operaciones después, **para** liberar espacio con 4–5 agentes a la vez sin perder trabajo por un descarte equivocado.

**Valor**: BR-07 (Must); NFR-01.

**Base y delta**: el descarte lo construye US-CKP-017 (DS-US-CKP-017): snapshot previo garantizado, borrado de worktree y rama, confirmación con default No, lista de lo que el snapshot no guarda, Deshacer inmediato que recrea el worktree, rechazo sin borrar si el snapshot falla y la tecla `D` de la TUI. Esta historia no repite nada de eso y añade cinco cosas:

1. La protección del agente vivo por los cinco estados de US-WTL-003, en lugar de «cualquier sesión presente»: el rechazo nombra al agente y su estado, y solo «terminado» permite descartar.
2. El informe del disco que ganó el descarte, medido en el disco.
3. En la confirmación, cuánto tiempo se puede restaurar y el origen de los ignorados copiados al crear el worktree.
4. La restauración de un worktree descartado durante la retención, aunque ya no sea la última operación.
5. El rechazo de un worktree ajeno con HEAD separado y la misma operación desde la CLI con salida JSON. La app la ofrece en US-DSK-016, sobre la misma operación.

## Criterios de Aceptación

**Escenario: Descarte de un agente terminado con informe de disco real**

Dado el worktree "feat-old" de "claude-1", en estado «terminado»
  Y `target/` copiado desde la semilla, con bloques compartidos con ella
Cuando el desarrollador descarta "feat-old" y confirma
Entonces el descarte se hace como en US-CKP-017
  Y GitRaptor informa el espacio que ganó el disco, medido como el espacio libre después menos el de antes, nunca la suma del tamaño de lo borrado

**Escenario: Agente vivo protegido en cualquiera de sus cuatro estados**

Dado el worktree "feat-wip" con "claude-2" en cada uno de estos estados: activo, esperando una decisión humana, esperando el CI o colgado
Cuando el desarrollador pide descartar "feat-wip"
Entonces la petición se rechaza indicando que "claude-2" ocupa "feat-wip" y en qué estado está
  Y no se borra nada ni se guarda ningún snapshot
  Y no se ofrece ninguna forma de forzar el descarte

**Escenario: Un agente colgado también protege su worktree**

Dado "claude-4" «colgado» desde hace 2 horas en "feat-d"
Cuando el desarrollador pide descartar "feat-d"
Entonces la petición se rechaza indicando que "claude-4" está «colgado» y que hay que terminarlo antes

**Escenario: La confirmación dice hasta cuándo se restaura y de dónde vienen los ignorados**

Dado "feat-old" con `target/` y `.env` copiados desde la semilla al crear el worktree
  Y la retención de la Time Machine en 30 días, el valor por defecto
Cuando el desarrollador pide descartar "feat-old"
Entonces la confirmación, además de la lista de US-CKP-017, dice que "feat-old" se puede restaurar durante 30 días
  Y que `target/` y `.env` no se guardan porque su original sigue en la semilla o en el repo principal

**Escenario: Restaurar dentro de la retención tras otras operaciones**

Dado "feat-old" descartado hace 10 días, con 1 commit sin integrar y 2 archivos sin commitear
  Y 5 operaciones hechas en el repo después del descarte
Cuando el desarrollador restaura "feat-old"
Entonces vuelven el worktree, la rama "feat-old" con su commit y los 2 archivos sin commitear
  Y las 5 operaciones posteriores no se deshacen

**Escenario: La rama ya existe con otra punta**

Dado "feat-old" descartado hace 10 días
  Y una rama "feat-old" creada después que apunta a otro commit
Cuando el desarrollador restaura "feat-old"
Entonces la restauración se rechaza indicando que la rama "feat-old" ya existe con otra punta
  Y no se escribe nada

**Escenario: Fuera de la retención**

Dado "feat-old" descartado hace 31 días, con la retención en 30 días
Cuando el desarrollador pide restaurarlo
Entonces GitRaptor indica que "feat-old" salió de la retención y no se puede restaurar

**Escenario: Worktree ajeno con HEAD separado**

Dado un worktree con HEAD separado que no creó GitRaptor, marcado «ajeno, no tocar»
Cuando el desarrollador pide descartarlo
Entonces GitRaptor lo rechaza porque está marcado «ajeno, no tocar» y no borra nada

**Escenario: Descarte y restauración desde la CLI con salida JSON**

Dado "feat-old" de "claude-1" en estado «terminado»
Cuando el desarrollador lo descarta desde la CLI con salida JSON y confirma
Entonces la respuesta trae el resultado del descarte, el espacio que ganó el disco y hasta qué fecha se puede restaurar
  Y una petición de descartar "feat-wip", con "claude-2" vivo, devuelve el rechazo con el agente y su estado

## Criterios de Certificación

- Verificar que el disco informado coincide con la diferencia de espacio libre antes y después, con artefactos compartidos con la semilla · *(método: medición en macOS con APFS, como el diario del 2026-11-09)*.
- Validar que el 100 % de los descartes se restauran dentro de la retención con rama y trabajo sin commitear, también con operaciones posteriores al descarte · *(método: prueba automatizada con repos temporales)*.
- Validar que ningún worktree con agente en uno de los cuatro estados vivos se descarta · *(método: prueba automatizada con sesiones simuladas)*.

## Requisitos Técnicos

- **Dev Spec:** [`dev-specs/US-WTL-002-descarte-reversible-disco.md`](../dev-specs/US-WTL-002-descarte-reversible-disco.md) — `DS-US-WTL-002` (status: spec-ready). Delta sobre DS-US-CKP-017, sin repetir el descarte.
- **Agente vivo**: solo un worktree sin agente vivo se descarta; estado desconocido falla cerrado; sin forzado. Sustituye X15 de DS-US-CKP-017 (W1–W4).
- **Disco**: espacio libre del volumen del worktree antes y después del borrado (`statvfs` de `rustix`, sin `unsafe`), marcado aproximado, o «no medido» con motivo (W5–W6).
- **Restauración**: `timemachine.undo` con `operation_id` (X9/X10 de DS-US-CKP-017) más comprobación de retención; el descarte se localiza con el método de lectura `timemachine.discards` (W9–W10).
- **Ajeno y autorización**: HEAD separado sin procedencia de GitRaptor se rechaza; nada por MCP ni por un agente; CLI con `--json` y confirmación humana (W11–W13).
- **Enmienda aplicada**: DS-US-CKP-017, «Enmienda (2026-11-21, US-WTL-002)». Propuesta: ADR-TMC-007 § 3 (previo de un descarte protegido hasta su retención).

## Diseño

- **Diseño (flujo/UX):** Pendiente de diseño (rechazo con agente y estado, línea de disco ganado y de retención en la confirmación y el resultado, restauración de un descarte en la TUI y en la app).

## Dependencias

- **De User Stories**: Requiere US-CKP-017 (el descarte base con Deshacer inmediato) y US-WTL-003 (los cinco estados que definen un agente vivo) · Relacionado US-WTL-005 (marca «ajeno, no tocar»), US-TMC-002, US-CKP-021 (Deshacer desde el historial), US-DSK-016 (descartar desde la app) · Bloquea US-WTL-004.
- **Cambio sobre la base**: la protección por estados sustituye la regla «cualquier sesión presente» de US-CKP-017 (X15 de DS-US-CKP-017). El Arquitecto decide en la Dev Spec de esta historia cómo se enmienda.
- **Preguntas abiertas que la historia sigue sin cerrarlas**: 10 (sin límite de descartes restaurables aparte de los 30 días), 13 (sin forma de forzar el descarte con agente vivo) y 15 (basta el aviso; ningún ignorado se guarda). La historia aplica la propuesta de CTX-WTL-001 para cada una; ninguna bloquea. Si Rene decide otra cosa, se ajustan los escenarios afectados.
- **Cross-cutting / transversales**: cómo se mide el disco ganado con bloques compartidos y cómo se localiza un descarte para restaurarlo: transversal (lo define el Arquitecto).
