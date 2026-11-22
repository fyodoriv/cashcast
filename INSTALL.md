# Install cashcast with your AI agent

This file is a playbook for an AI coding agent (Claude Code, Codex).
The repo is public; your money data never leaves your computer. A person
starts it with the prompt at the top of [README.md](README.md), or with one sentence:

> Install or update cashcast by following https://github.com/fyodoriv/cashcast/blob/HEAD/INSTALL.md

It works for a new machine and for an update. It sets up cashcast for a
user, and, when the person asks, for a contributor too. It ends by starting
the app with the person.

Agent: follow the steps below in order. Do not skip the plan or the
questions. Use your structured question tool (`AskUserQuestion` in Claude
Code) when you have one. Give every question its full context: what the
choice is, what each option does, and what changes for the person.

Privacy: never open, read, or print anything in `private/` (see the rules in
[AGENTS.md](AGENTS.md)). Learn about the person's data only from
`bin/cashcast` output.

## 1. Choose where the code goes

Ask: "Where should I put the cashcast code? It is a git clone; your money
data stays in its `private/` folder, which git never tracks." Options:

- `~/apps/cashcast` (recommended; matches the agentbrew and dotfiles defaults)
- `~/code/cashcast`
- Another folder (let the person type a path)

Then clone it, or update it if the folder already holds a clone. An update
keeps `private/` as it is: git never tracks it, and the setup below only
adds what is missing. If git asks for a password, the person is not signed
in to GitHub: run `gh auth login` first.

```bash
git clone https://github.com/fyodoriv/cashcast.git <DIR>   # or: git -C <DIR> pull --ff-only
```

## 2. Explain the plan

Tell the person, in your own short words:

- what they need: git, [Rust](https://rustup.rs) (cargo), jq, yq, sqlite3,
  and Node with pnpm; a few minutes (the
  first engine build is the slow part), and network for the first install,
- what will change: a `.venv` in the clone, the built engine in
  `engine/target`, the built web app, an owner-only `private/` folder, and a
  `~/.local/bin/cashcast` link. Nothing else outside the clone changes, the
  uv, cargo, and pnpm caches aside,
- that rerunning the setup is safe: it is idempotent,
- that no data leaves the computer unless they grant consent themselves,
- how to undo: delete the clone and the `~/.local/bin/cashcast` link.

If a tool is missing, `scripts/doctor.sh` names the install command for it.
Help install it first, then show the plan again.

## 3. Ask for preferences

Ask these together, in one round:

1. "Do you only use cashcast, or will you also change its code? A
   **contributor** setup also runs the fast gate once, so you know your
   machine can test a change." Use it (recommended) / Use and contribute.
2. "Who uses this copy? Each person gets their own folder, and a Canadian
   province sets their tax rules. For example `alex` in `ON`." Let the
   person type each name and province. Skip it if `bin/cashcast users`
   already lists them.

## 4. Install

Get a clear yes to the plan, then run from the clone:

```bash
cd <DIR> && scripts/setup.sh
```

It installs the web and engine dependencies, makes the local
folders, installs the git hooks, links `cashcast` on the PATH, and ends with
the doctor. On a failure, tell the person what failed and the fix line, and
offer the report in step 8.

Then add each person from step 3:

```bash
bin/cashcast users add <name> --province <XX>
```

### 4b. Contributor setup (only when the person contributes)

Run from the clone:

```bash
scripts/check.sh fast                             # the fast gate: lint, types, unit tests
```

Then point the person at [AGENTS.md](AGENTS.md) (the rules every change
follows) and `TASKS.md` (open work).

## 5. Check and fix

Run `scripts/doctor.sh`. Fix every failing line with the person: each line
names its fix. Run the doctor again until it ends with 0 failed ("doctor:
all checks passed"), or until the only failures need the person (a token, a
consent grant); say which.

If agentbrew is installed, run `agentbrew sync`, so `/cashcast` and the
other cashcast skills reach every agent.

## 6. Audit what the machine already has

List the money and budgeting skills and commands already on this machine
(for Claude Code: `~/.claude/skills/` and `~/.claude/commands/`; `agentbrew
status --verbose` shows where each comes from). Compare each one with
cashcast's `skills/`, `commands/`, and `.claude/skills/cashcast/`. For each,
propose one action and let the person choose:

- **adopt**: cashcast already does it; remove the local copy,
- **merge**: the local copy does something cashcast does not; offer to add
  it to cashcast (a skill, a command, or a CLI action),
- **keep**: it is not money work.

From now on, extend cashcast instead of building local copies.

## 7. Start cashcast for the first time

Run the app's one start command: `/cashcast` in the agent, or
`bin/cashcast` from the clone (`cashcast` once `~/.local/bin` is on the
PATH). It sets up what is missing, starts the local web app, reads new
exports from the inbox, and prints what needs the person now.

Read what it prints and walk the person through it, one item at a time:

- **The web app:** in an agent the link is not printed (it holds an access
  token). Tell the person to run `bin/cashcast web` in their own terminal
  and open the printed link.
- **Exports:** on a first run the inbox is empty. Ask which banks, cards,
  and apps they use, and show the steps with
  `bin/cashcast connectors show <service>`. Exports go in
  `private/users/<name>/inbox/`.
- **Consent:** when a report is withheld, give the person the exact
  `bin/cashcast consent grant <scope>` line to run in their own terminal.
  Never grant consent yourself.
- **Next:** say the next action the app printed.

## 8. Finish

Tell the person:

- `bin/cashcast actions` lists every action, and the web app's Actions view
  runs each one too,
- list anything in the install, the doctor, or the first run that could work
  better, and offer an issue or a PR for each one,
- if anything was wrong or confusing, the author is glad to hear it
  ([CONTRIBUTING.md](CONTRIBUTING.md)). Offer to report it:
  `agentbrew report --repo fyodoriv/cashcast` scrubs private details and
  shows the exact text before anything is posted; post only after the
  person says yes. Never include anything from `private/`. If they want to
  fix it themselves, offer to do it with them: a branch, the fix with a test
  and the docs, `scripts/check.sh`, then a PR.
