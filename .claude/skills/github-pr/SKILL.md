---
name: github-pr
description: Create a GitHub pull request, or work on an existing one — address review comments, push updates, reply to and resolve each unresolved thread.
---

# GitHub Pull Requests

Use this skill to **create** a new PR or to **respond to review
feedback** on an existing one.

## Creating a PR

Create PRs from the commit message — do not re-author the prose in
the PR form:

```sh
# Standard case
gh pr create --fill

# PR has no user-facing changes (skips the news-fragment check)
gh pr create --fill --label no-news-fragment-needed
```

`--fill` uses the most recent commit's subject as the PR title and
its body as the PR description, so a well-formed commit
(see [`../commit/SKILL.md`](../commit/SKILL.md)) produces a
well-formed PR automatically.

## Addressing review feedback on an existing PR

Use this whenever you are asked to address feedback, update a PR,
or "handle the review comments".

### Core rules

- **Reply to every unresolved review comment** — even when the fix
  is "done in commit abc123". Never leave a thread silently
  addressed.
- **Resolve each thread only after** (a) the fix is pushed and
  (b) a reply has been posted.
- **Push to update the PR.** Local commits alone do not count.
- No force-push to `main`. No `--no-verify`. No `git commit --amend`
  to rewrite commits that have already been pushed for review.
- Commit changes per [`../commit/SKILL.md`](../commit/SKILL.md) —
  including the mandatory `Co-authored-by:` trailer.

### Workflow (run these commands; substitute the placeholders in `<>`)

#### 1. Discover PR context

```sh
# Owner of the current clone
gh repo view --json owner --jq .owner.login

# Repo name of the current clone
gh repo view --json name --jq .name

# PR number for the current branch (fails if no PR exists)
gh pr view --json number --jq .number

# Or, if the PR number N was given explicitly, check it out:
gh pr checkout <N>
```

Capture `<OWNER>`, `<REPO>`, and `<N>` for the rest of the steps.

#### 2. List every unresolved review thread

The REST endpoint `/pulls/:n/comments` does **not** expose thread
resolution state. Always use GraphQL:

```sh
gh api graphql -f query='
  query($owner:String!,$name:String!,$number:Int!){
    repository(owner:$owner,name:$name){
      pullRequest(number:$number){
        reviewThreads(first:100){
          nodes{
            id isResolved isOutdated
            comments(first:50){
              nodes{ id databaseId author{login} path line body }
            }
          }
        }
      }
    }
  }' -F owner=<OWNER> -F name=<REPO> -F number=<N>
```

Filter client-side to `isResolved == false`. From each unresolved
thread you need:

- the thread `id` (GraphQL node id) → used to **resolve** the thread
  in step 5.
- the `databaseId` of the **first** comment in the thread → used to
  **reply** to the thread in step 4.

#### 3. Make the code changes, commit, push

Commit per [`../commit/SKILL.md`](../commit/SKILL.md). Then push:

```sh
git push                                           # already-tracked branch
git push -u origin "$(git branch --show-current)"  # first push of a new branch
```

Capture the commit SHA (`git rev-parse HEAD`) to reference in your
replies.

#### 4. Reply to each unresolved thread

Reply to the **first** comment in the thread (its `databaseId` from
step 2). GitHub threads your reply underneath automatically:

```sh
gh api --method POST \
  repos/<OWNER>/<REPO>/pulls/<N>/comments/<COMMENT_DATABASE_ID>/replies \
  -f body='Fixed in <SHA>. <optional short explanation>.'
```

Do **not** use `gh pr comment` — that posts a top-level PR comment,
which does not count as answering the review thread.

#### 5. Resolve each thread

Use the thread node `id` from step 2 (the GraphQL `id`, **not**
`databaseId`):

```sh
gh api graphql -f query='
  mutation($threadId:ID!){
    resolveReviewThread(input:{threadId:$threadId}){
      thread{ id isResolved }
    }
  }' -F threadId=<THREAD_NODE_ID>
```

The mutation returns `isResolved: true` on success.

#### 6. Verify

Re-run the step-2 query. Every thread you addressed should now have
`isResolved: true`. Any that remain unresolved either still need a
fix or were intentionally deferred — never silently skip one.

### Anti-patterns

- Posting a top-level PR comment (`gh pr comment`) instead of
  threading the reply on the review comment.
- Resolving without replying, or replying without resolving.
- Pushing before committing per [`../commit/SKILL.md`](../commit/SKILL.md)
  (skips the mandatory `Co-authored-by:` trailer).
- Force-pushing to shared branches to "clean up history" mid-review.
- Using `--amend` on commits that have already been pushed.
