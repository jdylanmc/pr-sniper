#!/usr/bin/env bash
# Publish the working-tree changes as a bot branch + PR (#291).
# main requires PR status checks, so the bot never pushes to main directly.
# GITHUB_TOKEN pushes trigger no push/pull_request runs: dispatch required CI.
# --ensure: publish nothing; reopen the PR and dispatch CI missing for the head.
set -euo pipefail
mode=publish
if [ "$1" = --ensure ]; then
  mode=ensure
  shift
fi
branch="$1"
title="$2"
body="$3"
if [ "$mode" = publish ]; then
  git config user.name "github-actions[bot]"
  git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
  git switch -C "$branch"
  git add -A
  git commit -q -m "$title" -m "$body"
  git push -q --force origin "HEAD:refs/heads/$branch"
fi
sha="$(git ls-remote origin "refs/heads/$branch" | cut -f1)"
pr="$(gh pr list --head "$branch" --base main --state open --json number --jq '.[0].number // empty')"
if [ -z "$pr" ] && ! gh pr create --head "$branch" --base main --title "$title" --body "$body"; then
  url="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/compare/main...$branch?expand=1"
  echo "::warning::GitHub Actions may not open PRs here; open $url"
  echo "Open the PR: $url" >>"$GITHUB_STEP_SUMMARY"
  exit 0
fi
for workflow in macos.yml windows.yml; do
  if [ "$mode" = publish ] || [ "$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs?head_sha=$sha" --jq '.total_count')" = 0 ]; then
    gh workflow run "$workflow" --ref "$branch"
  fi
done
