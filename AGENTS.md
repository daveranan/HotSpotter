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

### Sequential render-prompt workflow

When the user explicitly invokes `docs/render/orchestrator-prompt.md`, that workflow overrides the normal worker preference above only for the queued render migration:

- The root thread is an orchestrator only. It does not implement the queued prompt.
- Use exactly one render worker at a time. Prefer `render_prompt_worker` (Spark) for the initial implementation and its
  one remediation turn.
- Before spawning a full-prompt Spark worker, the root must classify prompt size. If the prompt contains multiple
  dependent implementation responsibilities that are likely to exhaust Spark's context, split it into two or three
  sequential slices and use fresh `render_prompt_slice_worker` agents. Only one slice worker may run at a time.
- Render workers must never reread and restart the whole task after context compaction. If Spark's context compacts
  before its first cohesive edit, or it reaches its read budget without enough information to edit safely, it must stop
  with `NEEDS_SPLIT` and return a two-or-three-slice proposal. The root then owns decomposition and dispatch.
- The root must actively enforce the Spark read budget rather than trusting self-reporting. A Spark slice gets at most
  two consolidated discovery tool calls; its third tool action must edit or return a terminal status. If it compacts,
  repeats discovery, or announces another read after that limit without `EDIT_APPLIED`, interrupt it immediately with
  `agents.interrupt_agent`.
- Interrupt immediately if a worker proposes guessed, approximate, placeholder, or "conservative default" semantics for
  missing authoritative contract data. If the missing contract cannot be fixed inside the prompt's edit boundary, mark
  the prompt blocked. Otherwise escalate the exact slice once to `render_prompt_slice_worker_terra`.
- Wait for that worker to finish, then perform one bounded root acceptance review against the current prompt before
  starting the next prompt.
- `docs/render/README.md` is the sole queue ledger. Only the root orchestrator updates it.
- The root review may inspect only the current prompt, its allowed files, the worker's reported changes, and directly
  relevant authoritative symbols. It must not create a new definition of done or perform a general audit.
- If that review finds concrete prompt violations, send one remediation brief to the same `render_prompt_worker` with
  `agents.followup_task`. If that thread is unusable because it exhausted context or failed before returning, one fresh
  `render_prompt_worker` may replace it for the same prompt. Never run both concurrently.
- In split mode, send the single remediation brief to the final integration slice worker instead. If it is unusable,
  spawn one fresh `render_prompt_slice_worker` with only the compact remediation brief and original prompt path.
- After Spark remediation, perform one final bounded re-review. If concrete implementation findings remain or Spark
  exhausted its context, escalate the same prompt once to `render_prompt_worker_terra` (`gpt-5.6-terra`, high). Give
  Terra the original prompt and the root's exact remaining findings. Never run Spark and Terra concurrently.
- Do not escalate environment, sandbox, approval, missing-hardware, malformed-prompt, or external-state blockers to
  Terra; stop and report those because a stronger model cannot resolve them.
- Perform one final bounded root review after Terra. If findings remain, verification is not real, or Terra is blocked,
  stop the queue. Do not launch another reviewer, another correction worker, Sol, or the next prompt automatically.
- Do not allow render workers to spawn subagents. All decomposition and dispatch remain in the root orchestrator.
