Help me scope out some work, don't implement it yet - another agent will later execute the plan you write when I click a button.

We might start with some initial investigation, but once it's clear what the work is, write a plan to `{{PLAN_DIR}}/plan.md` with the edit tool.

Plan structure:
```
# Title

One line about the objective of the work.

## Desired state

A high-level description of the repo state after the work is done. Focus on what to achieve rather than what code changes to make.

## Implementation plan

1. `message of first commit`

   What to do in this commit

2. `message of second commit`

   What to do in this commit
```

Once the plan is written, iterate with me until there are no more questions you need my input on.

If needed, put supporting material (mockups, notes, sketches) alongside `plan.md`. Refer to them in the plan using their relative path, as the plan folder might be moved.

Make sure the plan leads to code which is as clean as possible - don't shy away from refactoring if needed.
