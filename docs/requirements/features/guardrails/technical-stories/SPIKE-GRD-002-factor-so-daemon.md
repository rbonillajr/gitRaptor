---
id: SPIKE-GRD-002
title: "Factor de autenticación del SO invocado desde el daemon en los tres SO"
type: spike
status: draft
feature: guardrails
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-GRD-008, ADR-GRD-007, ADR-GRD-004, ADR-GRP-005]
  stories: [US-GRD-013, US-GRD-015]
  specs: []
ado:
  id: null
  url: null
tags: [guardrails, spike, factor-fuera-de-banda, presencia-humana, localauthentication, windows-hello, polkit, entrada-sintetica, fail-closed, unsafe-code]
---

## SPIKE-GRD-002: Factor de autenticación del SO invocado desde el daemon en los tres SO

> **Estado (2026-10-04)**: sin empezar. **Alcance inmediato: macOS.** Linux y Windows: **Pendiente: etapa de validación multiplataforma** (en Windows, además, el daemon todavía no tiene canal: ADR-GRP-005, Enmienda TS-GRP-004, punto 8). Mientras una plataforma no tenga su parte cerrada, US-GRD-013 y US-GRD-015 quedan en fail-closed en ella (ADR-GRD-008 § 7, OQ-GRD-008-9).

**Valor**: confirmar con pruebas reales que el daemon puede obtener una prueba de presencia del SO que un proceso del mismo usuario no fabrica, antes de que ADR-GRD-008 se convierta en el contrato de las Dev Specs de US-GRD-013 y US-GRD-015.

> Un SPIKE no lleva Dev Spec: su entregable es un Research Brief en `research/SPIKE-GRD-002-resultados.md`. Es un prototipo aislado (`spikes/os-presence-factor/`): un binario mínimo que reproduce las dos formas de arranque del daemon (autoarranque y arranque bajo demanda) y llama al diálogo del SO, sin código del daemon. **Depende de**: nada. **Valida**: ADR-GRD-008 § 2, § 3 y § 6; la enmienda de ADR-GRD-004 (OQ-GRD-008-8) no depende de él. **Bloquea**: la Dev Spec de US-GRD-013 y la de US-GRD-015 (la parte macOS basta para implementarlas con alcance macOS; el release en Linux y Windows espera a la parte pendiente).

### Pregunta

¿Puede el daemon, sin `unsafe` en el código del workspace, abrir el diálogo de autenticación del SO desde sus dos formas de arranque, ligarlo a un plazo y a un uso único, y obtener un resultado que ninguna entrada sintética del mismo usuario complete?

### Hipótesis

- **macOS**: LocalAuthentication funciona desde el LaunchAgent y desde el arranque bajo demanda con `setsid()` y entorno limpio, porque la sesión gráfica se hereda del padre y no de la sesión POSIX. Un daemon arrancado desde SSH no tiene sesión gráfica y queda en fail-closed aunque el humano tenga abierta la sesión gráfica.
- **Plazo y reutilización**: el daemon puede cancelar el diálogo a los 60 s, y dos evaluaciones seguidas piden dos gestos.
- **Método**: el SO no informa si se usó Touch ID, el reloj o la contraseña; la auditoría registra "no informado".
- **Entrada sintética**: `osascript` (con y sin el permiso de Accesibilidad), eventos sintéticos de teclado y ratón y `tmux send-keys` en la pty del humano no completan el diálogo; el campo de contraseña activa la entrada segura.
- **`unsafe`**: las llamadas directas de LocalAuthentication y de Windows Hello exigen `unsafe`, que el workspace prohíbe; una dependencia que encapsule el `unsafe` o un crate aislado con una excepción explícita es la única vía.

### Experimento

**macOS (ahora)**

- **Formas de arranque**: autoarranque como LaunchAgent; arranque bajo demanda con `setsid()` y entorno limpio lanzado desde Terminal.app, desde tmux, desde un IDE y desde un proceso hijo de Claude Code; y lanzado desde una sesión SSH con la sesión gráfica abierta. Anotar `canEvaluatePolicy` frente al resultado real de la evaluación en cada caso.
- **Primera llamada**: el fallo "UI activation timed out" tras iniciar sesión; si un reintento lo resuelve y cuánto tarda.
- **Binario**: sin firmar, con firma ad hoc y con la firma de release; con y sin bundle. Si el diálogo muestra el nombre del binario y el texto de la plantilla fija, y cómo se ven los parámetros saneados (SEC-GRD-06).
- **Plazo y uso único**: cancelar desde el daemon a los 60 s y comprobar que el diálogo desaparece; dos evaluaciones seguidas con contextos nuevos y sin periodo de reutilización.
- **Método informado**: qué datos devuelve el SO tras un éxito con Touch ID, con el reloj y con la contraseña.
- **Entrada sintética**: `osascript` con System Events, con y sin Accesibilidad concedida a la terminal; eventos sintéticos de teclado y ratón; `tmux send-keys` en la pty del humano que lanza el comando. Con la contraseña conocida por el atacante, si el campo acepta pulsaciones sintéticas.
- **Estados del equipo**: pantalla bloqueada, salvapantallas, otro usuario en primer plano (cambio rápido de usuario), tapa cerrada con monitor externo (sin Touch ID) y Mac sin Touch ID.
- **Concurrencia**: dos evaluaciones a la vez desde el mismo proceso.
- **Binding**: compilar la llamada con `objc2-local-authentication`, con `robius-authentication` y con un crate aislado con `unsafe`. Comparar la superficie de dependencias (SEC-GRD-15) y cómo encaja cada opción con `unsafe_code = "forbid"`.

**Linux — Pendiente: etapa de validación multiplataforma**

- **Agente**: qué agente de polkit atiende a un daemon bajo `systemd --user` y a uno arrancado bajo demanda, en GNOME (Wayland) y KDE (X11 y Wayland); qué sujeto conviene (el daemon o el proceso cliente, con su identidad) y en qué sesión cae.
- **Agente impostor**: si un proceso del mismo usuario puede registrarse como agente de la sesión y recibir la petición en una terminal.
- **`.policy`**: instalado con el comando que muestra `raptor doctor`; comprobación del contenido, del propietario y de los permisos en cada petición; sustitución de valores en el mensaje con `details`; distribuciones con `/usr` de solo lectura.
- **Sin sesión gráfica**: SSH, contenedor y WSL sin WSLg, en fail-closed.
- **Entrada sintética**: `xdotool` en X11 y su equivalente en Wayland.

**Windows — Pendiente: etapa de validación multiplataforma**

- **Diálogo**: Windows Hello desde la ventana transitoria del daemon; primer plano frente a las reglas de bloqueo del primer plano; con PIN, huella y reconocimiento facial (si el facial completa sin un gesto intencional).
- **Disponibilidad**: Windows 10, Hello sin configurar y sesión remota, en fail-closed.
- **Entrada sintética**: `SendInput` del mismo usuario y nivel de integridad.
- **Binding**: el mismo análisis de `unsafe` que en macOS, con el crate `windows`.

### Criterios de Éxito

- **Formas de arranque**: matriz confirmada de forma de arranque frente a resultado en macOS, lista para la disponibilidad que comprueba el daemon y para el mensaje de `raptor doctor`.
- **Ninguna entrada sintética completa el diálogo** en ningún caso probado. Cada intento queda documentado con su resultado.
- **Plazo, cancelación y uso único** verificados: el diálogo se cancela a los 60 s y cada acción pide un gesto nuevo.
- **Binding elegido** con su encaje en `unsafe_code = "forbid"`, como recomendación para la Dev Spec.
- **Coste**: el diálogo aparece en menos de 2 s tras la petición (⚠️ **ASSUMPTION**: objetivo de UX, sin NFR propio).
- **Vía de fracaso**:
  - Si una entrada sintética completa el diálogo en un SO, esa plataforma queda en fail-closed y se reabre ADR-GRD-008 § 2 para ella.
  - Si el arranque bajo demanda no puede abrir el diálogo, el factor solo está disponible con el autoarranque y `raptor doctor` lo explica; no se reabre el ADR.
  - Si no hay binding sin `unsafe` propio aceptable, se propone un ADR de excepción acotada a un crate aislado y auditado, antes de la Dev Spec.
  - Si el SO no permite cancelar el diálogo a los 60 s, el daemon anula la entrada pendiente a los 60 s y descarta cualquier resultado posterior.

### Time-box

⚠️ **ASSUMPTION**: 4 días para macOS. Linux y Windows, 3 días cada uno cuando llegue la etapa de validación multiplataforma.
