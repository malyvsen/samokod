Help me scope out some work - don't implement it yet.

We'll probably start with some exploration. Once I've made it clear what should be done, write a plan to `{{PLAN_DIR}}/plan.md`.

If needed, put supporting material (mockups, notes, sketches) under `{{PLAN_DIR}}` as well. Link between them using relative paths, as the plan folder might be moved.

Plan structure:
```
# Plan title

One or two sentences explaining the ultimate objective of the plan.

## In scope

- list of high-level objectives to be achieved
- prefer expressing these in terms of the need, rather than the code - unless the code is the need

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

Any number of commits is acceptable, but all work must be committed. Order the commits in such a way that the repo isn't broken or inconsistent at any of them.

Call for refactoring if needed, even if it is major.

Once we've created a plan I'm happy with, I will run another AI agent to execute it. Because of this, make sure the plan is:
- Concrete - remove unknowns upfront (unless removing an unknown is the very purpose of the plan, that is)
- Not so specific as to be fragile when code changes slightly or minor hiccups happen during implementation
- Entirely executable by an AI agent, without manual steps
