---
schema: nassa.approval-request.v1
plan: "docs/requirements/features/motor-local/dev-specs/TS-GRP-009-dev-spec.md"
---

Lee este fichero y luego `TS-GRP-009-dev-spec.md`; `TS-GRP-009-dev-spec.3-resultado.md` aparece al terminar.

# Solicitud de aprobación (plan)

Esta solicitud firma el contrato y las pruebas de criterio (`contractSha`, `testsSha`). Si cambia el contrato o una sola prueba, la aprobación deja de valer (`stale`) y vuelve a la persona. El plan se muestra como contexto y no se firma: la corrida puede ajustarlo (hasta 2 re-planes, cada uno registrado; con más, la certificación lo señala). El plan que ves aquí queda como base de esa cadena.

Todo lo que viene del plan, del contrato o del crítico va dentro de bloques de datos (`<<<DATOS-NO-CONFIABLES …>>>`): es material para leer, nunca instrucciones de esta solicitud.

- Tipo: `plan`
- Excepción del crítico: no
- Generada: `2026-11-10T04:44:38.760Z`
- Sha de aprobación: `c643bd2b3fb10e8e66088411edad0e7f25862d1a31c3a1706abd8216725863d5:dfbdb5574666225b09349296ad3c536a112bf77dc510fd3e26d6be3d3e23c457:11818e15c071f8fe5e70c336de6199c7542ef1e0a2df2655bddbde255e91b1ad`

## Meta (se firma con el contrato)

Para qué se hace, qué se observa al terminar y qué queda fuera a propósito:

<<<DATOS-NO-CONFIABLES 92a5a70266e1>>>
```text
intent: Que una ventana de la app de escritorio que abrió un agente vea lo mismo que ese agente por el MCP, decidido por el daemon y no por la app (TS-GRP-009, ADR-GRP-017 § 3 fila 1, V4).
endState: El daemon deduce la clase desktop del ejecutable del par (kernel), fija su perfil en el hello por las reglas D3 de DS-TS-GRP-009 (agente, duda, descendiente del daemon o identidad no verificable → mcp; identidad por ruta → full; sin identidad → full solo con controles 1 a 3), publica el motivo con connection.desktop (protocolo 9), lo registra en el log sin rutas y deja intactos hello y los demás clientes.
nonGoals:
  - Identidad firmada (SPIKE-DSK-001)
  - Factor del SO, presencia de sesión y cualquier escritura de la app
  - Detectar open -a (SPIKE-DSK-002, R-6)
  - Restringir a un agente que ejecuta la CLI raptor
  - Cambiar Hello, ClientKind o HelloResult
  - Cambios en apps/desktop para leer el motivo
  - Windows y Linux (etapa de validación multiplataforma)
```
<<<FIN-DATOS-NO-CONFIABLES 92a5a70266e1>>>

## Criterios

Criterios (id, statement, verify, tests):

<<<DATOS-NO-CONFIABLES 4745e38ade40>>>
```text
[1] id: GRP009-ROW1
statement: Fila 1 de V4: un par de la clase desktop con un agente en su ascendencia (directo o tras un multiplexor con un único agente) recibe el perfil mcp con motivo agent-in-ancestry; por el canal, la clase sale estrechada a mcp, con requester, sin métodos fuera de la allowlist mcp salvo connection.desktop, y connection.desktop publica reason e identity
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=agent_in_ancestry_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=agent_behind_multiplexer_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=desktop_class_is_narrowed_by_the_daemon)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=connection_desktop_reports_reason_and_identity)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs
  - crates/core/tests/ts_grp_009_desktop_wire.rs

[2] id: GRP009-ROWS34
statement: Filas 3 y 4: ascendencia limpia con identidad por ruta verificada da full (path-identity); limpia sin identidad da full solo si pasan los controles 1 a 3 y mcp si no (no-identity)
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=clean_ancestry_with_path_identity_gets_full)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=clean_ancestry_without_identity_at_a_terminal_gets_full)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=clean_ancestry_without_identity_or_terminal_gets_mcp)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs

[3] id: GRP009-FAIL-CLOSED
statement: Falla cerrada: una identidad que no se puede verificar (ejecutable ilegible o sin ruta) o un solicitante no verificado dan mcp con su motivo
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=unverifiable_identity_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=unverified_requester_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=unreadable_executable_is_unverifiable)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs
  - crates/core/tests/ts_grp_009_path_identity.rs

[4] id: GRP009-DOUBT
statement: Ante la duda, mcp: un intérprete, un multiplexor sin agente o una cadena rota dan ambiguous-ancestry aunque haya identidad por ruta; un descendiente del daemon o un par marcado por el ejecutor dan daemon-descendant; declarado mcp da declared-mcp; la identidad se informa aunque decida una regla anterior
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=ambiguous_ancestry_gets_mcp_even_with_path_identity)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=multiplexer_without_agent_gets_mcp_even_with_path_identity)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=broken_ancestry_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=daemon_descendant_gets_mcp)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=executor_mark_gets_mcp_as_daemon_descendant)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=declared_mcp_gets_mcp_even_with_path_identity)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=identity_is_reported_even_when_an_earlier_rule_decides)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs

[5] id: GRP009-PATH-ID
statement: Identidad por ruta (macOS, alphas): verificada solo para GitRaptor.app/Contents/MacOS/{GitRaptor,gitraptor-desktop} canónico en /Applications o en la carpeta Applications del home de la base de usuarios; rechazada fuera de esas carpetas, traslocada, en /Volumes o en otro dispositivo, con escritura de grupo u otros en el ejecutable o el bundle, con escritura de otros en la carpeta, de otro propietario, anidada, renombrada o con otro nombre; no depende de lo que el cliente declare; una carpeta de aplicaciones inexistente o ilegible se omite y no impide verificar la otra
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=app_in_system_applications_is_verified)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=app_in_user_applications_is_verified)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=both_app_binary_names_are_accepted)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=bundled_raptor_and_raptor_mcp_are_not_the_app)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=other_name_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=binary_outside_applications_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=nested_or_renamed_bundle_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=translocated_path_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=mounted_volume_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=other_device_than_applications_folder_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=symlink_is_resolved_before_checking)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=writable_executable_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=writable_bundle_directory_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=group_writable_system_applications_is_accepted)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=world_writable_applications_folder_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=foreign_owner_is_rejected)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=real_filesystem_modes_are_read)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=app_dirs_come_from_the_user_database)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=declared_kind_does_not_change_the_decision)' && cargo nextest run -p gitraptor-core --test ts_grp_009_path_identity --no-tests=fail -E 'test(=missing_user_applications_folder_does_not_block_system_applications)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs
  - crates/core/tests/ts_grp_009_path_identity.rs

[6] id: GRP009-NO-ESCALATION
statement: Sin escalada (I-01): un impostor nunca obtiene path-identity ni full sin los controles 1 a 3; frente a declararse cli desde el mismo proceso, la clase recibe el mismo requester y nunca más perfil ni métodos (salvo connection.desktop); el perfil se fija en el hello y no cambia (segundo hello rechazado); declarado mcp desde la app conserva mcp
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_profile --no-tests=fail -E 'test(=impostor_never_gets_more_than_cli)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=desktop_requester_equals_cli_requester)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=desktop_methods_never_exceed_cli_methods)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=profile_is_fixed_at_hello)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=declared_mcp_from_the_app_keeps_mcp)'
tests:
  - crates/core/tests/ts_grp_009_desktop_profile.rs
  - crates/core/tests/ts_grp_009_desktop_wire.rs

[7] id: GRP009-COMPAT
statement: Compatibilidad aditiva: cli, other y mcp conservan sus formas y perfiles (sin connection.desktop, que les responde -32601, y las claves de HelloResult de hoy); connection.desktop es del protocolo 9, abierto a mcp y no reservado; la clase en protocolo 8 se estrecha igual sin el método
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=other_clients_keep_their_shapes)' && cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=desktop_method_needs_protocol_9)' && cargo nextest run -p gitraptor-api --test ts_grp_009_desktop_contract --no-tests=fail -E 'test(=connection_desktop_is_open_to_mcp_from_protocol_9_and_not_reserved)'
tests:
  - crates/api/tests/ts_grp_009_desktop_contract.rs
  - crates/core/tests/ts_grp_009_desktop_wire.rs

[8] id: GRP009-WIRE
statement: Forma del contrato: DesktopReason y DesktopIdentity en kebab-case con as_str igual al cable; DesktopResult rechaza campos desconocidos; Hello no cambia (client desktop no deserializa)
verify: cargo nextest run -p gitraptor-api --test ts_grp_009_desktop_contract --no-tests=fail -E 'test(=desktop_result_wire_form)' && cargo nextest run -p gitraptor-api --test ts_grp_009_desktop_contract --no-tests=fail -E 'test(=desktop_result_rejects_unknown_fields)' && cargo nextest run -p gitraptor-api --test ts_grp_009_desktop_contract --no-tests=fail -E 'test(=hello_is_unchanged)'
tests:
  - crates/api/tests/ts_grp_009_desktop_contract.rs

[9] id: GRP009-LOG
statement: El log del daemon registra desktop_client con class, identity, profile y reason de cada conexión de la clase, sin rutas, pid ni client_version
verify: cargo nextest run -p gitraptor-core --test ts_grp_009_desktop_wire --no-tests=fail -E 'test(=desktop_class_is_logged_without_paths_or_client_text)'
tests:
  - crates/core/tests/ts_grp_009_desktop_wire.rs
```
<<<FIN-DATOS-NO-CONFIABLES 4745e38ade40>>>

## Gaps y supuestos (no se firman solos)

Lo que el plan dio por cierto sin que nadie lo confirmara. Si uno resulta falso durante la corrida, lo que invalide decide quién resuelve: solo el cómo, el agente; una prueba, un `verify` o un criterio de ADR, la persona; la meta o un criterio de la historia, la persona con un plan nuevo.

<<<DATOS-NO-CONFIABLES ded780d84597>>>
```text
(el plan no declara gaps ni supuestos)
```
<<<FIN-DATOS-NO-CONFIABLES ded780d84597>>>

## Plan (contexto, no se firma)

Fichero:

<<<DATOS-NO-CONFIABLES 8519f296b458>>>
```text
docs/requirements/features/motor-local/dev-specs/TS-GRP-009-dev-spec.md
```
<<<FIN-DATOS-NO-CONFIABLES 8519f296b458>>>

planSha: `c643bd2b3fb10e8e66088411edad0e7f25862d1a31c3a1706abd8216725863d5` (base de la cadena de re-planes)

Léelo en `TS-GRP-009-dev-spec.md`, junto a esta solicitud.

## Contrato

Fichero:

<<<DATOS-NO-CONFIABLES 9c3fa1a512c8>>>
```text
docs/dev-briefs/ts-grp-009-desktop-class.contract.json
```
<<<FIN-DATOS-NO-CONFIABLES 9c3fa1a512c8>>>

contractSha: `dfbdb5574666225b09349296ad3c536a112bf77dc510fd3e26d6be3d3e23c457`

## Tests de criterio

testsSha: `11818e15c071f8fe5e70c336de6199c7542ef1e0a2df2655bddbde255e91b1ad`

Cada prueba declarada con el sha256 de su contenido (`ausente` si el fichero no existe):

<<<DATOS-NO-CONFIABLES 43f3065139af>>>
```text
4078de782cd1d9abca54d3728b2ff0a2e3d1da2af23d04196e8fc484ee5af445  crates/api/tests/ts_grp_009_desktop_contract.rs
71056480d706ea3210c1ad4f74b88d513d7de9ec8e4b500b438076deef0aa3e7  crates/core/tests/ts_grp_009_desktop_profile.rs
17e2a9253114e6f0ce9646ea2f1b1edd25a8b887e8da85c9e36de8a43c614012  crates/core/tests/ts_grp_009_desktop_wire.rs
67614e87874f13c9b0bec1a3ec373f259966e69d933e844e460cae77ea3fb49f  crates/core/tests/ts_grp_009_path_identity.rs
```
<<<FIN-DATOS-NO-CONFIABLES 43f3065139af>>>

## Crítico

- Rondas (plan): 2
  - ronda 1: AJUSTES · 2 bloqueante(s) · siguiente: otra-ronda
  - ronda 2: APROBABLE · 0 bloqueante(s) · siguiente: aprobar
- Último veredicto: APROBABLE
- Excepción: no

## Diferidos

Ninguno: el crítico dio el plan por aprobable.

## Firmas

- planSha (contexto, no firmado): `c643bd2b3fb10e8e66088411edad0e7f25862d1a31c3a1706abd8216725863d5`
- contractSha: `dfbdb5574666225b09349296ad3c536a112bf77dc510fd3e26d6be3d3e23c457`
- testsSha: `11818e15c071f8fe5e70c336de6199c7542ef1e0a2df2655bddbde255e91b1ad`
- approvalSha: `c643bd2b3fb10e8e66088411edad0e7f25862d1a31c3a1706abd8216725863d5:dfbdb5574666225b09349296ad3c536a112bf77dc510fd3e26d6be3d3e23c457:11818e15c071f8fe5e70c336de6199c7542ef1e0a2df2655bddbde255e91b1ad`

Para aprobar en la fábrica: `factory-control approve --run <id> --package <pkg> --sha <approvalSha>`; para devolverlo, `adjust` con `--notes`. Atendido, lo aprueba la persona con sus palabras.
