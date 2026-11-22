# Evals

Evals prove the assistant still does its job after a prompt, skill, or model change.

- `cases/`: one YAML file per skill, holding one or more cases as YAML
  documents (`---`), synthetic data only. Each case has the input, the
  expected outcome, and whether the assistant should act (`should: act`) or
  hold back (`should: refrain`). Write cases from real failures first.
- `rubric.md`: binary pass/fail criteria. The same rubric drives the
  assistant's own feedback and the judge, so feedback quality is under test too.
- Graders: prefer code checks (regex, schema). Use an LLM judge only for open
  quality, one criterion per call, on the local route in `config/models.yaml`.
- Two suites: `regression` (must stay near 100%; run on every prompt or skill
  change) and `capability` (new, harder cases; may fail).
- Runner: `evals/run.sh`. `CHECK_EVALS=1 scripts/check.sh` runs it (never
  cached) and fails when it is missing.
- `grader: cli` cases run `bin/cashcast` on a synthetic user. Add
  `no_python: true` to hide every `python*` from PATH (and unset `BASH_ENV`):
  `rust-only-start.yaml` proves `bin/cashcast doctor` needs no Python.

## Judge cases

A `grader: judge` case holds a synthetic `response`: the answer the assistant
should give to `input`. The judge grades that response.

- **Where it runs**: only the `local` route in `config/models.yaml`, an Ollama
  model on this computer (`qwen3:14b`, at `http://127.0.0.1:11434`). The runner
  refuses any other route. It never calls a hosted or paid model.
- **What it sends**: the case's `input`, `fixture`, `response` and one
  criterion. All of it comes from `evals/`. Nothing from `private/` is read or sent.
- **One criterion per call**: each `expect` line, then each rubric criterion the
  case names in `rubric:` (for example `rubric: [safe-with-money]`). The call
  uses temperature 0 and asks for JSON `{"pass": true|false, "reason": "..."}`.
  A reply without a boolean `pass` fails the case; it is never read as a pass.
- **When Ollama is down** or the model is missing, the judge cases are skipped
  with one line, and the run still exits 0 when the code-graded cases pass.
  If the judge stops answering mid-run (a call waits up to 5 minutes), the
  remaining cases are skipped the same way.
  Agent sandboxes often block local ports, so run it in Terminal:
  `ollama serve`, then `ollama pull qwen3:14b` once.

## Owner labels and agreement

Trust the judge only after you check it against your own labels.

1. Open `labels.yaml`. For each case, read its `input` and `response` in
   `cases/`, then replace `"owner: fill in"` with `pass` or `fail`. Label before
   you look at the judge's verdict. Each judge case also has a deliberately bad
   answer (`bad_response`, label id `<id>-bad`). The eval run expects the judge
   to fail it, so a judge that always says PASS loses.
2. Run `evals/run.sh --agreement`. It runs the judge cases only and prints
   `judge vs owner labels: k of n agree`, plus each disagreement. It counts
   only the labels you filled in.

Goal: the judge agrees on at least 5 of 7 good answers and 5 of 7 bad ones.
Below 4 of 7, keep these cases human-graded and use the judge only to spot changes
between runs.
