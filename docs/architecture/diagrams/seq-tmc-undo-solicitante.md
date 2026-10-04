# Secuencia — Undo con solicitante, confirmación, política y solape

> Covers: US-TMC-002, US-TMC-012, US-TMC-013, US-TMC-014, US-TMC-015, US-TMC-021 · ADR-TMC-005, ADR-TMC-002 § 3-4, SEC-TMC-03, SEC-TMC-15. Orden: allowlist y canal, validación, conjunto, regla base, confirmación, política, solape, precondiciones. Cada rechazo termina el flujo: el repo no cambia y la petición queda en el oplog.

```mermaid
sequenceDiagram
  autonumber
  participant C as raptor undo (CLI) o raptor-mcp
  participant D as Daemon: solicitante y permisos
  participant M as Motor: atribución vigente
  participant P as Política de Guardrails
  participant L as Planificador
  participant A as Aplicador
  participant O as Oplog
  participant R as Repo del usuario

  C->>D: undo (repo y worktree salen del cwd del llamante)
  D->>D: identificador no reutilizable, ascendencia con hora de inicio y multiplexores
  alt canal MCP y (repo fuera de la allowlist o solicitante "sin atribuir")
    D->>O: rechazada (incondicional, previo a todo)
    D-->>C: rechazo con motivo
  else petición admitida a evaluación
    D->>L: última operación no deshecha del worktree
    L->>M: actor vigente de lo que se deshace
    M-->>L: actores
    alt agente X y el conjunto no es solo de X
      D->>O: rechazada (trabajo ajeno)
      D-->>C: rechazo con motivo
    else "sin atribuir" (CLI/TUI), hay trabajo de un agente y el cliente no supera los controles
      Note over D: sin terminal, bajo un agente o con un multiplexor compartido con un agente
      D->>O: rechazada (sin confirmación posible)
      D-->>C: rechazo con motivo
    else permitido por la regla base (con reto si hace falta confirmar)
      opt "sin atribuir" con trabajo de un agente
        D-->>C: plan + reto de un solo uso (conexión, proceso, hash del plan, 60 s)
        C-->>D: respuesta al reto
        D->>D: valida el reto, que se invalida si el plan cambió
      end
      D->>P: evaluar política (solo puede denegar)
      alt política deniega o reto inválido
        D->>O: rechazada (política o reto)
        D-->>C: rechazo con motivo
      else permitido
        L->>L: solape por archivo y por ref
        alt solape
          L->>O: rechazada por solape
          L-->>C: cambios en conflicto con su actor y momento
        else sin solape
          L->>R: precondiciones (operación de Git en curso, locks ajenos)
          alt precondición falla
            L->>O: rechazada (Git ocupado u operación en curso)
            L-->>C: rechazo con motivo
          else listo
            L->>L: ya empujado (refs remotas locales, sin red)
            L->>A: aplicar destino
            A->>O: snapshot previo garantizado + pasos en el diario
            A->>R: index.lock propio, refs con valor esperado, intercambio atómico por archivo, índice
            A->>O: terminada (solicitante, destino, avisos)
            A-->>C: hecho + aviso "sigue en el remoto" si aplica
          end
        end
      end
    end
  end
```
