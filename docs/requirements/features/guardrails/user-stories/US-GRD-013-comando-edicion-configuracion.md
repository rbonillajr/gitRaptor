---
id: US-GRD-013
title: "El desarrollador cambia la configuración con un comando sin perder lo que editó a mano"
type: us
status: draft
priority: medium
created: 2026-10-04
updated: 2026-10-28
feature: guardrails
related:
  context:
    - CTX-GRD-001
  rules:
    - BR-GRD-001
  stories:
    - US-GRD-010
    - US-GRD-014
  specs:
    - DS-US-GRD-013
tags:
  - guardrails
  - comando-edicion
  - configuracion-tres-niveles
  - nfr-01
---

# US-GRD-013: El desarrollador cambia la configuración con un comando sin perder lo que editó a mano

## Descripción

**Como** desarrollador orquestador, **quiero** editar cualquiera de los tres niveles de la configuración con un comando que respete los niveles admitidos y no pise mis cambios a mano, **para** ajustar las reglas sin abrir archivos ni arriesgar la configuración.

**Valor**: editar la configuración es seguro y explica lo que no se puede hacer (Q27 de motor-local, NFR-01).

## Reglas cubiertas

BR-CONS-006 (comando, Guardrails: no pisa cambios a mano, escritura atómica y recuperable, sin commit) · BR-VAL-001 (rechaza un valor en un nivel no admitido) · BR-CONS-001 (rechaza relajar una regla del equipo) · BR-AUTH-001 (un agente no relaja con el comando) · Q-GRD-16 (rama base inexistente) — ver [business-rules.md](../business-rules.md)

## Dependencias

- **Historias**: US-GRD-010 (precedencia), US-GRD-014 (rama base como valor del equipo).
- **Externas**: **desbloqueada el 2026-10-04** al aceptarse [ADR-GRD-008](../../../../architecture/decisions/ADR-GRD-008-factor-autenticacion-fuera-de-banda.md) (factor de autenticación del sistema operativo fuera del canal del agente, gate de Q-GRD-19). Su Dev Spec ya existe (DS-US-GRD-013, 2026-10-28) con un corte fail-closed aceptado por el PO: todo lo que piden los escenarios se implementa sin esperar, y relajar responde `factor-unavailable` sin escribir (Q-GRD-19). Solo relajar con el factor espera a los resultados de [SPIKE-GRD-002](../technical-stories/SPIKE-GRD-002-factor-so-daemon.md) (parte macOS; Linux y Windows: Pendiente: etapa de validación multiplataforma, con fail-closed mientras tanto), mediante una enmienda de la Dev Spec; US-GRD-015 espera al mismo SPIKE. El trinquete de los niveles personales de Q-GRD-32 (enmienda de ADR-GRD-004, aplicada el 2026-10-04) se construye en una historia aparte del PO. P8 ya no la bloquea (ADR-GRP-007). Endurecer con el comando no exige el factor. Donde el factor no está disponible, relajar no se ofrece (ADR-GRD-008, OQ-GRD-008-1 y 7). Distinguir al humano: transversal (ADR-GRD-008; R-GRD-3). Por Q-GRD-17 y Q-GRD-20, un endurecimiento del comando en el nivel de equipo se aplica al commitearlo en el worktree; una relajación, solo cuando llega a la rama principal y el desarrollador la confirma en su máquina (Q-GRD-21).
- **Transversal**: Windows, macOS y Linux; pruebas de interrupción (NFR-12).

## Criterios de Aceptación

**Escenario: El comando cambia un valor, no hace commit y el cambio se aplica al commitearlo**

Dado el repo "demo" con push permitido en la configuración del equipo commiteada
Cuando el desarrollador cambia push a "denegar" en el nivel de equipo con el comando, en el worktree "feat-x"
Entonces el cambio queda en el working tree sin ningún commit nuevo
  Y el siguiente push desde "feat-x" se sigue ejecutando hasta que el desarrollador commitea el cambio
  Y después de commitearlo se deniega

**Escenario: El comando rechaza un valor en un nivel que no lo admite**

Dado el repo "demo"
Cuando el desarrollador fija con el comando la rama base "develop" en su perfil
Entonces el comando lo rechaza, explica que la rama base solo se define en la configuración del equipo y no escribe nada

**Escenario: El comando no pisa cambios hechos a mano**

Dado el repo "demo" con una ruta prohibida añadida a mano en la configuración del equipo y sin commitear
Cuando el desarrollador cambia con el comando el permiso de rebase
Entonces el comando no escribe y avisa de que hay cambios que no hizo él
  Y la ruta añadida a mano sigue en la configuración

**Escenario: Una escritura interrumpida no corrompe la configuración**

Dado el repo "demo" con una configuración del equipo válida
Cuando la escritura del comando se interrumpe a mitad
Entonces la configuración es la anterior o la nueva, completa
  Y el desarrollador puede recuperar el valor anterior al cambio

**Escenario: Fijar una rama base que no existe pide confirmación**

Dado el repo "demo" sin la rama "release"
Cuando el desarrollador fija con el comando la rama base "release" en el nivel de equipo
Entonces el comando avisa de que "release" no existe y solo la guarda si el desarrollador lo confirma

**Escenario: Relajar una regla desde el canal de un agente se rechaza**

Dado el repo "demo" con force-push denegado por el equipo
Cuando un agente ejecuta el comando para permitir force-push
Entonces el comando lo rechaza, no escribe nada y el intento queda en el registro

## Requisitos Técnicos

Resumen de [DS-US-GRD-013](../dev-specs/US-GRD-013-comando-edicion-configuracion.md); el detalle y las decisiones están allí.

- **Comando**: `raptor config set|unset|add|remove <KEY> [<VALUE>] --level team|profile|local [--path] [--yes]`, con una lista cerrada de claves de Guardrails. Método reservado `guard.config-edit` del daemon: un agente no lo usa, ni para endurecer (BR-AUTH-004), y su intento queda auditado.
- **Corte respecto a SPIKE-GRD-002**: todo lo que piden los escenarios se implementa ya. Relajar, en cualquier nivel, responde `factor-unavailable` sin escribir (fail-closed, ADR-GRD-008) hasta que el SPIKE dé resultados; entonces una enmienda de la Dev Spec cambia ese único punto por el flujo del factor. US-GRD-015 comparte la dependencia.
- **Cambios que no hizo él**: en el nivel de equipo solo escribe si el archivo coincide con el `HEAD` del worktree o con su última escritura (marca en el almacén del repo). En los niveles personales parte del archivo tal como está y rechaza si cambia entre la lectura y la escritura.
- **Escritura**: copia del archivo anterior en el perfil, temporal + `fsync` + `rename` atómico, sin commit. Puntos de corte para las pruebas de interrupción (NFR-12).
- **Rama base inexistente**: avisa y pregunta (`[y/N]`), y sin respuesta no escribe (Q-GRD-16).

## Diseño y Dev Spec

- **Diseño:** no aplica (textos del comando: CLI, i18n en/es).
- **Dev Spec:** [DS-US-GRD-013](../dev-specs/US-GRD-013-comando-edicion-configuracion.md) (2026-10-28; decisiones del orquestador validadas por el Arquitecto y el PO; relajar con factor espera a SPIKE-GRD-002).
