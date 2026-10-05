Help me scope out some work - don't implement it yet.

We'll probably start with some exploration. Once I've made it clear what should be done, write a plan to `{{PLAN_DIR}}/plan.md`.

If needed, put supporting material (mockups, notes, sketches) under `{{PLAN_DIR}}` as well. Link between them using relative paths, as the plan folder might be moved.

Plan structure:
```
# Plan title

One or two sentences explaining the ultimate objective of the plan.

## In scope

- [ ] todo list of high-level objectives to be achieved
- [ ] prefer expressing these in terms of the need, rather than the code - unless the code is the need

## Out of scope

- list of things to not bother doing

## Steps

### 1. Commit `first commit message`

A free-form description of what to do in the first commit.

### 2. Commit `second commit message`

A free-form description of what to do in the second commit.

### 3. Some non-commit step, if needed

A free-form description of what to do in the non-commit step.
```

Any number of commits is acceptable, down to a single one if that's all it takes. Order the commits in such a way that the repo isn't broken or inconsistent at any of them.

Make sure the plan is concrete - remove unknowns upfront (unless removing an unknown is the very purpose of the plan, that is). If possible, make sure it is entirely executable by an AI agent.

Call for refactoring if needed, even if it is major.
