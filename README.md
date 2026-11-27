# Samokod

> An opinionated AI coding tool optimized for parallel execution.

## Manifesto

Working with one AI agent at a time is easy on your brain, but inefficient - you spend a lot of time watching it work. Coordinating multiple agents at a time is painful, because you need to consolidate their changes somehow. Samokod takes care of this by enforcing structure in your process.

The idea is to split your work into plans which get executed in parallel. They shouldn't overlap too much - that will slow your agents down - but a little overlap is fine.

Each plan goes through the following stages:

1. **Scoping** - you talk with an AI agent about what to do, it writes a plan
2. **Execution** - an agent carries it out in its own worktree
3. **Rebasing** - an agent rebases the result onto the latest state of your repo's checked-out branch, making sure to account for changes which happened on the base branch during execution
4. **Evergreening** - an agent makes sure that the code looks as if it was designed this way from the start
5. **Finalization** - Samokod fast-forwards your branch and removes the worktree

Your time is almost exclusively spent on scoping. Once you approve a plan for execution, Samokod takes care of the remaining AI coordination. The resulting Git history is linear, reads clearly, and the code is free from the results of agents lacking coordination, fighting each other or taking shortcuts.

## Requirements

- A Git repository to work on
- [OpenCode](https://opencode.ai) on your `PATH` - Samokod drives it as a backend, it owns all agent state
