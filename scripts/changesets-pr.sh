#!/usr/bin/env bash
# Publish the release commit as a bot branch + PR (#291).
# main requires PR status checks, so the bot never pushes to main directly.
# GITHUB_TOKEN pushes trigger no push/pull_request runs: dispatch required CI.
set -euo pipefail
branch="$1"
title="$2"
body="$3"
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git switch -C "$branch"
git add -A
git commit -q -m "$title" -m "$body"
git push -q --force origin "HEAD:refs/heads/$branch"
pr="$(gh pr list --head "$branch" --base main --state open --json number --jq '.[0].number // empty')"
if [ -z "$pr" ] && ! gh pr create --head "$branch" --base main --title "$title" --body "$body"; then
  url="$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/compare/main...$branch?expand=1"
  echo "::warning::GitHub Actions may not open PRs here; open $url"
  echo "Open the PR: $url" >>"$GITHUB_STEP_SUMMARY"
  exit 0
fi
for workflow in macos.yml windows.yml; do
  gh workflow run "$workflow" --ref "$branch"
done
