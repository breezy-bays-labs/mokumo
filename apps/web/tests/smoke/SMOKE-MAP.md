# SMOKE-MAP — E2E Harness Traceability

> Each row maps a SMOKE-NN ID to its spec path (or unit test path / follow-up issue),
> disposition, and notes. This file is the canonical traceability artifact for the
> manual smoke checklist.
>
> CI lint (`tests/smoke/ci-lint.sh`): ripgrep every `SMOKE-\d{2}[a-z]?` in the codebase
> and `tests/manual/MANUAL_SMOKE.md`; assert each resolves to a row here and each
> row's path or issue exists.
>
> - `automated` + `covered-by-unit` dispositions must resolve to an existing path.
> - `manual` + `needs-computer-use` dispositions must resolve to `MANUAL_SMOKE.md`
>   or a filed GitHub issue URL.

| SMOKE-NN  | Title (brief)                             | Path                                      | Disposition        | Notes                                                                                   |
| --------- | ----------------------------------------- | ----------------------------------------- | ------------------ | --------------------------------------------------------------------------------------- |
| SMOKE-01  | SIGTERM drain                             | tests/smoke/lifecycle.spec.ts             | automated          | describe.serial; test-scoped harness                                                    |
| SMOKE-02  | SIGKILL→banner                            | tests/smoke/ws-disconnect-sigkill.spec.ts | automated          | describe.serial; test-scoped harness                                                    |
| SMOKE-03  | restart→reconnect                         | tests/smoke/lifecycle.spec.ts             | automated          | describe.serial; same-port restart + page.reload()                                      |
| SMOKE-04  | stop() 12 s, no zombie                    | tests/smoke/lifecycle.spec.ts             | automated          | test-scoped harness                                                                     |
| SMOKE-05  | port conflict                             | tests/smoke/port-management.spec.ts       | automated          |                                                                                         |
| SMOKE-06  | port fallback                             | tests/smoke/port-management.spec.ts       | automated          |                                                                                         |
| SMOKE-07  | port exhaustion                           | tests/smoke/port-management.spec.ts       | automated          | test.fixme — see follow-up issue                                                        |
| SMOKE-08  | mDNS retry                                | tests/manual/MANUAL_SMOKE.md#SMOKE-08     | manual             | D8 deferred; clock injection needed in Rust                                             |
| SMOKE-09a | tray quit decision                        | crates/core/src/tray/                     | covered-by-unit    | Decision logic in crates/core unit tests                                                |
| SMOKE-09b | tray quit wiring                          | tests/manual/MANUAL_SMOKE.md#SMOKE-09b    | needs-computer-use | M1-gated AC (see MANUAL_SMOKE.md)                                                       |
| SMOKE-10a | quit dialog count                         | crates/core/src/tray/                     | covered-by-unit    | Decision logic in crates/core unit tests                                                |
| SMOKE-10b | quit dialog OS window                     | tests/manual/MANUAL_SMOKE.md#SMOKE-10b    | needs-computer-use | M1-gated AC (see MANUAL_SMOKE.md)                                                       |
| SMOKE-11a | tray icon state                           | crates/core/src/tray/                     | covered-by-unit    | Decision logic in crates/core unit tests                                                |
| SMOKE-11b | tray icon OS tray                         | tests/manual/MANUAL_SMOKE.md#SMOKE-11b    | needs-computer-use | M1-gated AC (see MANUAL_SMOKE.md)                                                       |
| SMOKE-12  | close-to-tray pref                        | tests/smoke/frontend-state.spec.ts        | automated          | capture harness.dataDir before stop; reuse on restart                                   |
| SMOKE-13  | first-launch nudge                        | tests/smoke/frontend-state.spec.ts        | automated          |                                                                                         |
| SMOKE-14  | null LAN addr                             | tests/smoke/frontend-state.spec.ts        | automated          |                                                                                         |
| SMOKE-LT  | liveness timer (SIGSTOP/partition→banner) | crates/core/ (N94 unit test)              | covered-by-unit    | CQO-4: Playwright cannot simulate network partition; heartbeat covers silent-death path |
