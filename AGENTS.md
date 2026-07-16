Keep the task scoped to the requested outcome.
Do not widen the search or add agents unless it could change the result.
Run the cheapest targeted check that could show the current approach is wrong.
Stop optional investigation when more work stops changing the answer.
Always run the required safety checks and acceptance criteria, using one focused verification command when the task allows it.

## Subagents

Default to doing the work in the root thread without subagents.

Do not automatically launch an explorer, tester, reviewer, Sol reviewer, or general audit worker. A separate tester or reviewer must not create a new definition of done after the assigned implementation is already green.

Use at most one worker by default, and only for a concrete bounded implementation slice with:

- concrete scope
- acceptance criteria
- relevant files or subsystem
- exactly one targeted verification command

Prefer `phase_worker_fast` for normal delegated implementation. It uses Terra (`gpt-5.6-terra`) with high reasoning effort and should inspect only directly relevant files, implement the assigned behavior, run the specified verification command, make at most one correction pass, rerun the same command, and stop.

Use `phase_worker_hard` only after `phase_worker_fast` fails and the parent explicitly requests escalation for the reported blocker. It uses Sol (`gpt-5.6-sol`) with high reasoning effort.

Do not launch `phase_worker_hard` for sandbox or tool-infrastructure failures. Restart or repair the execution environment before retrying the slice.

The parent thread owns synthesis, integration, and the final user response. After a worker reports success, do not run a parent diff audit, broad test sweep, production build, browser inspection, documentation pass, or unrelated cleanup unless the user explicitly asked for it or the focused verification exposes a concrete regression.
