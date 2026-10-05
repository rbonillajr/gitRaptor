---
id: INF-CKP-001
title: "Esqueleto de la TUI: cliente del canal, bucle TEA, saneado único y gate de 100 ms"
type: inf
status: draft
feature: cockpit
domain: GRP
priority: critical
complexity: medium
created: 2026-10-04
updated: 2026-10-04
related:
  adrs: [ADR-CKP-003, ADR-GRP-004, ADR-GRP-005, ADR-GRP-011, ADR-GRP-013]
  stories: [TS-GRP-004, INF-GRP-001, INF-GRP-002, TS-CKP-004]
  specs: []
ado:
  id: null
  url: null
tags: [cockpit, tui, ratatui, tea, canal, saneado, sec-12, latencia, nfr-04, i18n, ci, br-04, dep-ckp-6, dep-ckp-9]
---

## INF-CKP-001: Esqueleto de la TUI con cliente del canal, saneado único y gate de 100 ms

**Valor**: todas las pantallas del Cockpit se construyen sobre el mismo bucle, el mismo cliente y el mismo saneador, y una regresión de latencia de la TUI rompe el CI nombrando la etapa que se pasó.

### Descripción

**Como** Arquitecto
**Quiero** el esqueleto de la TUI de `apps/cli` con su cliente del canal, su modelo TEA, el punto único de saneado y la conducción sin pantalla desde el banco
**Para** que cada historia de BR-04 a BR-07 añada solo su vista y su lógica, con la fuente única, SEC-12 y los 100 ms p95 del Cockpit (ADR-GRP-011) impuestos por construcción (ADR-CKP-003)

> Dev Spec: `dev-specs/INF-CKP-001-esqueleto-tui.md` | Pendiente
>
> **Depende de**: TS-GRP-004 (biblioteca cliente y contrato; el esqueleto necesita N1 a N7 de ADR-CKP-003 § 4, **pendiente, dueño: worker del canal (TS-GRP-004)**), TS-CKP-004 (tema: ningún widget usa literales) e INF-GRP-002 (banco y suscriptor sin pantalla). El gate de 100 ms es el de la enmienda E2 de ADR-GRP-011, aplicada el 2026-10-04. **ADRs**: ADR-CKP-003 § 1 a § 8, § 10 y § 12, que la Dev Spec sigue punto por punto; ADR-GRP-011 § 3 (reloj monótono). **Seguridad**: SEC-01 y L-06 (par del canal), SEC-08 (resync), SEC-12. **Habilita**: todas las historias de BR-04 a BR-07 y la CLI de solo lectura (Q-CKP-20).

### Alcance Técnico

- **Crear** en `apps/cli` el target de biblioteca interno con los módulos de cliente, modelo, presentación y TUI, sin crates nuevos (ADR-CKP-003 § 12).
- **Implementar** el bucle de un solo hilo dueño del modelo, con colas de entrada y del motor, la entrada drenada primero y un render coalescido por iteración (ADR-CKP-003 § 3).
- **Implementar** el modelo TEA con actualización y vista puras, comandos ejecutados fuera de la actualización y reloj de vista inyectado (ADR-CKP-003 § 2).
- **Implementar** el cliente del canal: arranque coherente por ámbito, descarte de duplicados, resincronización ante un hueco o un `resync`, reconexión con espera creciente y cambio de repo (ADR-CKP-003 § 4).
- **Comprobar** el par del canal antes del handshake (directorio del socket del uid y uid del par) o reutilizar esa comprobación si la biblioteca cliente ya la hace; dónde vive es **pendiente, dueño: worker del canal (TS-GRP-004)** (L-06).
- **Implementar** la máquina de estados de la conexión, que desactiva las escrituras fuera de "En vivo" y las que no permite la capa resuelta en el handshake.
- **Implementar** el adaptador de ingesta como único punto donde el texto no confiable pasa a texto saneado, con un tipo saneado que solo ese módulo construye (ADR-CKP-003 § 8).
- **Crear** el catálogo i18n tipado en/es, de modo que una traducción ausente no compile (ADR-CKP-003 § 10).
- **Crear** la tabla única acción ↔ teclas que alimenta la entrada, las pistas de teclado y la ayuda.
- **Implementar** la inicialización y la restauración de la terminal, también ante un pánico, y la suspensión que reutilizan el editor y `Ctrl-Z`.
- **Instrumentar** `t_client_recv` y `t_render` con el reloj monótono común y un histograma local por etapa que nunca sale de la máquina (ADR-CKP-003 § 6).
- **Conducir** la misma aplicación sin pantalla desde el banco de INF-GRP-002 contra el daemon real, con el gate del Cockpit y el aviso de feedback por tecla.
- **Crear** el microbanco sintético sin daemon que corre en cada PR que toque `apps/cli`.
- **Registrar** las comprobaciones estáticas de CI: solo el módulo del subcomando del daemon importa el motor; la TUI no importa la capa de Git ni la de políticas; ningún widget recibe texto del contrato sin sanear.
- **Fuera de alcance**: las vistas y los flujos de cada historia (lista, alertas, grafo, acciones); la presentación de los estados de conexión y del arranque del daemon, que es observable (historia de BR-04, Q-CKP-22); `--plain`, la detección del modo de accesibilidad y la CLI de solo lectura (historias dueñas); el tema (TS-CKP-004); el lanzador del editor (historia de Q-CKP-9); las preferencias (DEP-CKP-11).

### Plan de Verificación

#### Pruebas Automatizadas

- **Secuencia**: pruebas de propiedades sobre la actualización: un duplicado se descarta; un hueco lleva a resincronizar sin aplicar nada posterior; un `resync` y una reconexión rehacen la instantánea. Ninguna petición de escritura cambia la réplica sin un evento del motor.
- **Latencia (banco de INF-GRP-002 con el daemon real)**: p95 del Cockpit (`t_client_recv` → `t_render`) > 100 ms **falla** (E2 de ADR-GRP-011); extremo a extremo p95 ≥ 500 ms **falla** (gate de ADR-GRP-011); feedback por tecla p95 ≥ 100 ms, aviso. El informe nombra la etapa: decodificar, aplicar o pintar.
- **Microbanco**: ráfaga de 1.000 archivos con 10 worktrees y 55 pares; el número de frames queda acotado y una tecla no espera detrás de la ráfaga más que el presupuesto de aplicación.
- **Saneado (SEC-12)**: fuzzing y propiedades: la salida del saneador no contiene C0, C1, DEL ni controles bidi. El corpus incluye secuencias OSC 52, cambio de título, borrado de pantalla, U+009B y RLO. Snapshot de una rama maliciosa: secuencia visible y buffer sin ESC.
- **Fronteras**: las comprobaciones estáticas fallan si la TUI importa el motor, la capa de Git o la de políticas, o si un widget recibe texto sin sanear.
- **Par del canal (L-06)**: con el directorio del socket en 0755 o de otro uid, o un servidor falso de otro uid, la TUI pasa a "Canal rechazado" sin enviar el handshake.
- **Sin perfil (suite de INF-GRP-001)**: con las carpetas de datos y configuración del perfil sin permiso de lectura, la TUI funciona igual.
- **i18n**: todo código del contrato tiene mensaje en y es; la build falla si falta uno.
- **Terminal**: un pánico provocado en la vista deja la terminal restaurada.
- **Varias TUIs**: dos aplicaciones sin pantalla conectadas al mismo daemon ven el mismo estado.

#### Verificación Manual / Sandbox

- En macOS, abrir la TUI en dos emuladores de terminal durante una ráfaga del banco y revisar el histograma local por etapa.
- Consola de Windows y terminales de Linux: **Pendiente: etapa de validación multiplataforma**.
