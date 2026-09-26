I launched you in a git worktree whose branch is `{{WORKTREE_BRANCH}}`.

Rebase `{{WORKTREE_BRANCH}}` on top of `{{MAIN_BRANCH}}` by running `GIT_SEQUENCE_EDITOR="perl -pi -e 's/^pick /edit /'" git -C {{WORKTREE_PATH}} rebase -i {{MAIN_BRANCH}}`.

You may edit any file in the worktree, not just conflicted ones: each rebased commit must stay complete and faithful to its original intent. For example, if a commit mass-renamed something, make sure its rebased version also renames all uses of the renamed symbol which have been introduced on `{{MAIN_BRANCH}}`. Make sure that every commit resulting from the rebase passes checks standard for this repo.

If it is unclear what a commit's intent was, refer to `{{PLAN_MD_ABS_PATH}}`, which documents an implementation plan which was carried out in this worktree.
