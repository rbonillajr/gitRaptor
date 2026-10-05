---
id: SEQ-CKP-ARRANQUE-TUI
title: "Arranque de la TUI: daemon bajo demanda, instantánea N y suscripción N+1, reconexión"
type: diagram
status: expanded
domain: GRP
feature: cockpit
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-003, ADR-GRP-005, ADR-GRP-011, ADR-GRP-013]
  stories: [INF-CKP-001, TS-GRP-004, US-CKP-001, US-CKP-003]
---

# Secuencia — Arranque de la TUI: daemon bajo demanda, instantánea N y suscripción N+1, reconexión (BR-04)

> Covers: BR-04 (BR-CKP-WF-004, CONS-001) · INF-CKP-001, TS-GRP-004 · ADR-CKP-003 § 3 a § 5, ADR-GRP-005 § 3 y § 4, ADR-GRP-013 (secuencia por repo) · US-CKP-001, US-CKP-003.

La TUI nunca embebe el motor ni lee Git o el perfil (Q-CKP-22). Si no hay daemon, lo arranca la biblioteca cliente de `crates/api`. La vista se llena con una instantánea con secuencia N por ámbito (global y repo seleccionado) y una suscripción desde N+1: un duplicado se descarta y un hueco o un `resync` llevan a pedir otra instantánea. Al perder el canal, la réplica se conserva marcada como desconectada y las escrituras se desactivan hasta volver a "En vivo". Antes de cada handshake, el cliente comprueba el par del canal (L-06); si la biblioteca cliente ya lo hace, se reutiliza. La forma de estos mensajes (N1 a N7 de ADR-CKP-003 § 4) y dónde vive la comprobación del par son **pendiente, dueño: worker del canal (TS-GRP-004)**.

```mermaid
sequenceDiagram
  autonumber
  actor U as Desarrollador
  participant T as TUI · hilo principal (modelo y render)
  participant C as TUI · hilo del canal (cliente)
  participant L as crates/api · biblioteca cliente
  participant S as Gestor de servicios o entorno limpio
  participant D as raptor daemon (canal y motor)

  U->>T: raptor (entrada y salida son TTY)
  T->>T: inicializa la terminal, estado "Conectando" en la barra
  C->>L: conectar
  L-->>C: sin daemon
  C->>T: Msg "Arrancando el motor…" (escrituras desactivadas)
  L->>S: arranque bajo demanda (allowlist de entorno, SEC-10)
  S->>D: raptor daemon
  alt daemon listo dentro del tiempo máximo (⚠️ ASSUMPTION, 5 s)
    C->>C: comprueba el par antes del handshake (directorio del socket del uid con 0700, uid del par igual al propio, L-06)
    alt el par no cuadra
      C->>T: "Canal rechazado" con qué revisar, sin enviar nada
    else par correcto
      L->>D: handshake (versión de protocolo y del catálogo)
      D-->>L: versión, solicitante y capa resueltos de la conexión, autoarranque registrado o no
    end
    opt versión incompatible
      L->>L: sustituye el daemon solo si este es el binario instalado, si no muestra instrucciones
    end
  else falla o se agota
    C->>T: Msg "Motor no disponible" con raptor daemon, raptor daemon enable y tecla de reintento
  end

  C->>D: repo de esta ruta (el daemon canonicaliza, la TUI no lee Git)
  D-->>C: id del repo observado (o el último usado, Q-CKP-1)
  C->>D: instantánea global y del repo
  D-->>C: instantánea con secuencia N por ámbito (t_client_recv)
  C->>D: suscribir desde N+1
  C->>T: Msg instantánea
  T->>T: "Sincronizando" (esqueleto), ingesta y saneado, "En vivo", escrituras activas según la capa ("actúas como X")
  opt sin autoarranque registrado
    T->>U: aviso una vez por sesión, sin activarlo
  end

  loop stream de eventos
    D-->>C: evento con secuencia s
    alt s menor o igual que la última aplicada
      C->>C: descarta (duplicado)
    else s es la última más 1
      C->>T: Msg evento, se aplica y se pinta coalescido
    else s mayor que la última más 1 (hueco)
      C->>T: "Resincronizando", la vista se conserva marcada como desactualizada
      C->>D: instantánea nueva y suscripción desde su N más 1
    end
  end
  opt resync del daemon (cliente lento, SEC-08)
    D-->>C: resync con su causa
    C->>D: mismo camino que un hueco
  end

  D--xC: canal perdido (daemon parado o caído)
  C->>T: "Reconectando", "desconectado desde hh:mm", escrituras desactivadas
  loop reintento con espera creciente (⚠️ ASSUMPTION, de 250 ms a 5 s, sin límite mientras la TUI esté abierta)
    C->>L: conectar o arrancar bajo demanda
  end
  C->>C: comprueba el par otra vez (L-06)
  L->>D: handshake
  C->>D: instantánea con secuencia N' y suscripción desde N'+1 (el MVP no reanuda desde una secuencia)
  C->>T: Msg instantánea, "En vivo"

  Note over T,C: El módulo del subcomando del daemon es el único de apps/cli que importa el motor, y corre en su propio proceso. Comprobación estática de INF-CKP-001.
```

Linux (servicio de usuario) y Windows (pipe con nombre, consola): **Pendiente: etapa de validación multiplataforma**.
