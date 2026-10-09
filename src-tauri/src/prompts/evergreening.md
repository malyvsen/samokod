# Introduction
You are working in the git worktree at {{WORKTREE_PATH}}, whose branch is {{WORKTREE_BRANCH}}. The plan at {{PLAN_MD_ABS_PATH}} was just executed here against the target branch {{TARGET_BRANCH}}. The commits below, oldest first, are what a coding agent did in order to carry it out:
{{COMMITS}}

# Objective
The objective is to make sure that the codebase reads as if it had been written with the current logic and features in mind from the start. This applies not only to the code, but also to tests, documentation, and anything else the codebase contains. Remove all convoluted naming, roundabout logic and pointless abstractions. Consider both the contents of files and their structure - renaming files, splitting or joining them, changing the directory structure etc. are all on the plate.

# Constraints
Your work should be a pure refactor - behaviour should not change, except for fixing bugs or improving performance.

# What to watch for
Coding agents tend to produce minimal-diff changes with backward compatibility in mind, but this is detrimental to the long-term maintainability of the codebase. For example:
- An agent wanted to create a file called `gardening` where it would put cabbage-planting logic.
- The agent discovered that a file with that name already existed and contained irrigation and soil-prep logic.
- It decided to create a new file called `cabbage-planting` and put the cabbage-planting logic there.
- In that case, you should consider creating a `gardening` directory, place the original file into it while splitting it into `irrigation` and `soil-prep`, and move `cabbage-planting` in there too.

Coding agents also tend to add patches on top of existing content rather than changing the underlying structure and possibly removing unneeded content when that would be more appropriate. For example:
- An agent was told to ensure cabbages always have water.
- It decided to add logic which finds where cabbages grow and adds sprinklers there.
- It did not notice that, once this is done, sprinklers must simply be present in the entire garden, so the code stayed peppered with special cases when it could be much simplified.
- In this case, you should consider removing the special cases and simply ensuring that sprinklers are present in the entire garden.

One more thing which coding agents notoriously do, especially when writing documentation but also when naming things, is to assume that future readers will be familiar with their conversation context, which of course they won't. For example:
- An agent was told to implement a more efficient sprinkler system.
- When drafting a plan for the implementation, it called the new system "sprinkler-system-v2".
- When implementing the plan, it kept referring to "v2" even though future readers will not need to know which iteration it is.
- In this case, you should consider removing the mentions of "v2".

Note that when this prompt tells you to consider doing something, it does not necessarily mean you should do it. It is your judgement call whether this is actually beneficial to the overall maintainability of the codebase and conformance to its coding standards. To help you in your decision, you may look at the history of the codebase - what the direction seems to be, where bugs appeared historically, what was easy or difficult to change, which pieces of it seem coupled and which seem independent, etc.

# Scope
The scope is this worktree. The commits listed above and the plan show what was done and what was intended. Consider the edited code plus adjacent and related content.

# When done
Use TODOs to track progress in detail - at least one TODO per cleanup area, but more are encouraged.

When every cleanup is done, summarize what you did and stop. Leave the worktree clean - do not start landing.
