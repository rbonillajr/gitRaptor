---
id: SEQ-GRD-DECISION-HOOK
title: "Decisión de Guardrails en un hook de Git"
type: diagram
status: expanded
domain: GRP
feature: guardrails
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRD-001, ADR-GRD-002, ADR-GRD-003, ADR-GRD-004, ADR-GRD-006, ADR-GRD-007]
  stories: [US-GRD-001, US-GRD-002, US-GRD-005, US-GRD-006]
---

# Secuencia — Decisión de Guardrails en un hook de Git (US-GRD-001, US-GRD-002, US-GRD-005, US-GRD-006)

Muestra un agente que hace un force-push con Git crudo en un repo protegido:

- el dispatcher, que solo contiene constantes;
- la evaluación con entorno por allowlist, contra un daemon autenticado;
- la decisión en el daemon: directorio común, `git` más cercano, actor, suelo de D6 y decisión pura;
- el encadenado en Rust con el hook previo cuando la decisión permite.

También muestra el modo degradado.

```mermaid
sequenceDiagram
  autonumber
  participant AG as Agente (Claude Code u otro)
  participant G as git (sistema)
  participant D as Dispatcher pre-push<br/>(solo constantes)
  participant EV as raptor hook · evaluación<br/>(entorno por allowlist)
  participant DM as Daemon · módulo guardrails
  participant P as crates/policy · evaluar()
  participant ST as Almacén del repo (perfil)
  participant CH as raptor hook · encadenado<br/>(Rust, sin shell)
  participant PH as Hook previo (usuario o gestor)

  AG->>G: git push --force origin feat-x
  G->>D: pre-push con argumentos y refs por stdin
  D->>EV: constantes: binario, hook, id del directorio común, ruta del canal, id de instancia, directorio de estado del perfil, hook previo (sin HOME ni PATH)
  EV->>EV: parser en streaming, normaliza refs (NFC, mayúsculas) y transiciones
  EV->>DM: conecta por la ruta del canal (constante, sin leer el perfil)
  EV->>EV: verifica que el servidor es el binario instalado (firma o huella)
  DM-->>EV: handshake con el id de instancia del perfil
  EV->>EV: compara con la constante (si no coincide, modo degradado instance-mismatch)
  EV->>DM: evaluar(operación normalizada, id del directorio común)
  DM->>DM: cwd del git más cercano en un worktree de ese directorio común
  DM->>DM: actor desde el git más cercano (S4)
  DM->>DM: configuración: mínimo + suelo (rama principal) + endurecimientos de worktree y personales
  DM->>P: evaluar(operación, contexto, configuración)
  P-->>DM: Decision{effect: deny, reasons: [minimum.force-push · minimum]}
  alt token válido para esta transición, git hijo directo del raptor solicitante
    DM->>ST: kind=exception
    DM-->>EV: appliedEffect: allow, exception: applied
  else sin excepción
    DM->>ST: kind=denial (agregada si se repite)
    DM-->>EV: appliedEffect: deny
  end
  alt deny
    EV-->>D: salida distinta de 0 y mensaje con plantilla fija, parámetros etiquetados
    D-->>G: salida distinta de 0, el hook previo no se ejecuta
    G-->>AG: push rechazado con el motivo
  else allow
    EV-->>CH: veredicto por el canal que fija el dispatcher
    CH->>PH: mismos argumentos y stdin, entorno original sin el token
    PH-->>CH: código de salida
    CH-->>G: el mismo código
  end

  Note over EV,DM: Daemon no arrancable o de otra instancia: modo degradado más estricto (servidor no auténtico: deny en refs gobernadas)<br/>(mínimo forzado, el equipo solo endurece, sin niveles personales, sin excepciones),<br/>entrada al spool y ventana registrada por el daemon al volver
  Note over D: raptor ausente o error interno: fail-closed solo en pre-push, pre-rebase y borrado de ramas,<br/>el resto pasa con aviso (ADR-GRD-001 § 3)
```
