# Secuencia — Recuperación tras un kill -9 a mitad de un undo

> Covers: US-TMC-019, US-TMC-016 (purga interrumpida) · ADR-TMC-003 § 3 y § 6, ADR-TMC-002 § 3. El daemon muere mientras aplica; al volver no reanuda ni revierte por su cuenta: marca la operación, libera solo su propio lock (excepción declarada a BR-TMC-CONS-004, sin tocar contenido) y avisa.

```mermaid
sequenceDiagram
  autonumber
  participant A as Aplicador (daemon)
  participant O as Oplog y diario
  participant S as Almacén de snapshots
  participant R as Repo del usuario
  participant N as Nuevo daemon
  participant C as Siguiente cliente del worktree

  A->>O: intención · snapshot previo completo · lista; broken on purpose
  A->>O: aplicando(paso 3: index.lock propio, ruta e inodo)
  A->>R: crea index.lock
  A->>O: aplicando(paso 5: refs)
  A->>R: transacción de refs
  A->>O: aplicando(paso 6: archivos)
  A-xR: intercambia archivos...
  Note over A: kill -9 (proceso muerto)
  N->>O: verificar la cadena de hashes del oplog antes de aceptar operaciones
  N->>O: snapshots pendientes → descartados
  N->>S: borrar refs del almacén sin fila completa (huérfanas)
  N->>O: operación en "aplicando" → interrumpida
  N->>R: ¿index.lock con el inodo anotado y sin hijo vivo?
  alt es el propio
    N->>R: lo borra (excepción: libera un lock propio, no toca contenido)
  else ajeno o hijo vivo
    Note over N,R: no se toca (espera acotada si hay hijo vivo)
  end
  N->>O: aviso pendiente de interrupción
  C->>N: se conecta desde el worktree
  N-->>C: aviso: undo interrumpido, "raptor undo" vuelve al estado previo
  C->>N: raptor undo
  N->>R: aplica el snapshot previo de la operación interrumpida (con su propio snapshot previo)
  N->>O: terminada
```
