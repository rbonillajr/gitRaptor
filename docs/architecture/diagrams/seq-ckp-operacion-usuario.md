---
id: SEQ-CKP-OPERACION-USUARIO
title: "Operación de usuario desde la TUI: preparar, decidir, proteger y ejecutar"
type: diagram
status: expanded
domain: GRP
feature: cockpit
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-002, ADR-CKP-003, ADR-TMC-004, ADR-TMC-005, ADR-GRD-003, ADR-GRD-006, ADR-GRD-007]
  stories: [TS-CKP-002, TS-CKP-003, INF-CKP-001, TS-TMC-004]
---

# Secuencia — Operación de usuario desde la TUI: preparar, decidir, proteger y ejecutar (BR-07)

> Covers: BR-07 (BR-CKP-ELIG-002, WF-002, WF-003, WF-008, CONS-002, CONS-004, AUTH-001, AUTH-002) · TS-CKP-002, TS-CKP-003, INF-CKP-001 · ADR-CKP-002 § 1 a § 6, ADR-TMC-004 § 1, ADR-TMC-005 § 1 y § 3, ADR-GRD-003 § 4 a § 6, ADR-GRD-007. Todavía no hay historias de usuario del Cockpit.

El desarrollador integra en la base confirmada el trabajo del worktree W2 (`merge-into-base`). La TUI no escribe: pide un plan, muestra lo que el daemon devuelve y pide ejecutar ese plan por la misma conexión. El daemon fija el solicitante y la capa, toma la decisión de Guardrails antes de cualquier efecto, toma el snapshot previo y lanza `git` como padre directo tras registrarlo. Los hooks de Guardrails de ese `git` heredan la decisión, y todo proceso que desciende de él actúa como el solicitante del plan. Incluye la rama denegada y la excepción consciente.

```mermaid
sequenceDiagram
  autonumber
  actor U as Desarrollador
  participant T as TUI raptor
  participant D as Daemon · ejecutor (solicitante, capa, plan, cerrojo)
  participant P as crates/policy · decisión
  participant TM as Time Machine · operación protegida
  participant G as git (hijo directo del daemon)
  participant H as Hook de Guardrails
  participant M as Daemon · motor y canal

  U->>T: tecla de integrar sobre W2
  T->>T: estado pendiente visible en la siguiente iteración (sin UI optimista)
  T->>D: prepare(merge-into-base, W2, secuencia vista, variables de sesión declaradas)
  D->>D: solicitante por ascendencia y capa fijada por el daemon (cockpit solo para "sin atribuir" con terminal y sin agente)
  alt el solicitante es un agente (capa mcp), también desde la TUI
    D-->>T: rechazo "operación no disponible para este solicitante" (merge y descarte no tienen marca MCP)
  else capa cockpit
    D->>D: precondiciones en orden (operación en curso, HEAD separado, base confirmada, destino limpio y sin sesión presente, locks de Git, grafts)
    D->>D: plan con valores esperados e identidad del repo, avisos (⚡, sesión Activa "hasta el commit X"), trabajo afectado y huella (con solicitante, capa y versión del catálogo)
    D->>P: evaluar(operación normalizada, actor, capa cockpit) como vista previa
    P-->>D: decisión previa
    D-->>T: planId ligado a esta conexión, plan, avisos, decisión previa, confirmaciones requeridas y reto si hay trabajo ajeno

    alt decisión previa: permitida
      T->>U: ConfirmPrompt con todos los motivos (default No)
      U->>T: confirma
      T->>D: execute(planId, códigos de aviso aceptados, respuesta al reto)
    else decisión previa: denegada (p. ej. regla de rama protegida)
      T->>U: PolicyBanner ⛔ con la regla y el nivel
      alt el desarrollador no sigue o el plan caduca
        D->>D: al cerrarse el plan, una sola entrada "denial" con capa cockpit
      else excepción consciente (Q-CKP-15)
        U->>T: pide la excepción
        T->>D: comando reservado de excepción ligado al planId y a la huella
        alt fallan los controles 1 a 3 o el cliente desciende de un hijo del ejecutor
          D-->>T: rechazo, entrada "exception-rejected"
        else controles superados
          D->>M: anuncio reserved-action-pending a todos los clientes
          Note over D,M: Ventana cancelable (⚠️ ASSUMPTION heredada, 10 s, S-CKP-3) antes de tomar el cerrojo, para no retener el repo
          alt cancelada en la ventana
            D-->>T: cancelada, entrada "exception-cancelled"
          else ventana vencida
            T->>D: execute(planId, excepción aplicada)
          end
        end
      end
    end

    D->>D: acepta el planId solo si es de esta conexión y no caducó
    D->>D: cerrojo de escritura del repo (si está ocupado, "en cola" visible en todos los clientes)
    D->>TM: intención en el oplog
    D->>D: re-resuelve solicitante y capa, rehace el plan, compara la huella y revalida el repo (dev e inode, gitdir bidireccional)
    alt huella, solicitante, capa o repo distintos
      D-->>T: rejected "el estado cambió", sin efectos
    else todo coincide
      D->>P: re-evaluar (la decisión que cuenta, antes de cualquier efecto)
      alt denegada sin excepción aplicada
        D-->>T: rejected con la regla
      else permitida, o denegada con la excepción de este plan
        D->>TM: snapshot previo garantizado
        alt el snapshot falla
          TM-->>D: error
          D-->>T: aborted, el repo no cambia
        else previo hecho
          TM-->>D: comprobante del previo (único que permite lanzar)
          D->>G: crea el git detenido (repo y working tree explícitos, argv fijo, sin shell ni terminal, entorno por allowlist)
          D->>D: barrera de arranque, registra identidad del hijo y transiciones del plan
          D->>G: libera el hijo (si el registro falló, lo mata y da failed-unchanged)
          G->>H: hook de la transición de la base
          H->>D: evaluar (git más cercano)
          D-->>H: decisión ya tomada (hijo registrado, transición coincidente), sin snapshot por hook
          H-->>G: permite y encadena el hook previo del usuario
          opt un hook del usuario abre el canal
            Note over G,D: Se resuelve como el solicitante del plan. No puede pedir comandos reservados, confirmaciones, la excepción, Cancelar ni operaciones del catálogo. Sus git nietos se evalúan de nuevo con ese actor.
          end
          G-->>D: código de salida
          D->>D: lee el estado resultante con gitoxide
          D->>TM: registro del resultado
          D->>D: cierre del plan, como mucho una entrada de registro ("exception" si se usó)
          D-->>T: done, stopped (conflicto) o failed, con la salida de Git saneada (solo a esta conexión de capa cockpit)
        end
      end
    end
    D->>D: libera el cerrojo
    M-->>T: eventos del motor (W2 y la base cambiaron) y fin de operación, a todos los clientes
    T->>T: pinta el resultado y el toast "u: deshacer"
  end

  Note over D,G: Nunca push ni fetch, sin red. Si un hook o la firma piden interacción, Git falla (sin terminal) y el resultado es stopped o failed-unchanged. Capa cockpit sin tiempo máximo, con Cancelar.
  Note over T,D: Por MCP: mismo flujo con capa mcp y solo operaciones con marca MCP (commit, rebase atómico, crear worktree con plantilla, snapshot). Tiempo máximo, sin salida de Git y rechazo de lo que exige confirmación, del trabajo ajeno y de la excepción.
```

Decisión del orquestador (2026-10-04), validada por Arquitecto: la ventana de la excepción consciente corre **antes** de tomar el cerrojo del repo y queda ligada al `planId` y a la huella; si el plan cambia durante la ventana, el `execute` se rechaza. ADR-CKP-002 § 4 no fija ese orden; se confirma en la Dev Spec de la historia de la excepción. Windows (capa `cockpit`, barrera de arranque, confirmación de trabajo ajeno) y Linux: **Pendiente: etapa de validación multiplataforma**.
