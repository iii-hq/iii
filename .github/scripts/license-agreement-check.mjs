import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export const CHECKBOX_TEXT =
  "I license my contributions to this repository under Apache 2.0, and I have all necessary rights over the code I am contributing.";
export const STATUS_CONTEXT = "license-agreement";
const BOT_LOGINS = new Set(["github-actions[bot]"]);

export function hasCheckedCheckbox(prBody = "") {
  const escaped = CHECKBOX_TEXT.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^- \\[x\\] ${escaped}\\s*$`, "im").test(prBody);
}

export function isInContributorsFile(username, content = "") {
  return content
    .split("\n")
    .some((line) => line.trim() === `- @${username}` || line.trim().startsWith(`- @${username} `));
}

export function evaluateAgreement({ prBody = "", inContributorsFile = false } = {}) {
  const checkboxChecked = hasCheckedCheckbox(prBody);
  const acknowledged = inContributorsFile || checkboxChecked;

  return { acknowledged, checkboxChecked, inContributorsFile };
}

export function buildCommitMessage(prAuthor) {
  return `docs: add @${prAuthor} to contributors.md\n\n@${prAuthor} agrees to license contributions to iii under Apache 2.0.`;
}

export function buildPendingComment(prAuthor) {
  return [
    "## License agreement required",
    "",
    `@${prAuthor}, to contribute to this repository please confirm that you license your changes under Apache 2.0 and that you have all necessary rights over the code you are contributing.`,
    "",
    "Copy the following into your PR description and check the box:",
    "",
    "```markdown",
    `- [ ] ${CHECKBOX_TEXT}`,
    "```",
    "",
    "When you check the box, this workflow will automatically add you to [contributors.md](contributors.md) with the following commit:",
    "",
    "```",
    buildCommitMessage(prAuthor),
    "```",
    "",
    "Once added, all future PRs from your account will pass this check automatically.",
    "",
    "Alternatively you can add your github username as a new line to the end of `contributors.md` in the repository root.",
  ].join("\n");
}

export function buildSatisfiedComment(prAuthor) {
  return [
    "## License agreement recorded",
    "",
    `@${prAuthor}, your agreement has been recorded and you have been added to [contributors.md](contributors.md). All future PRs from your account will pass this check automatically.`,
  ].join("\n");
}

export function findStickyComment(comments = []) {
  return comments.find(
    (comment) =>
      BOT_LOGINS.has(comment.user?.login) &&
      (comment.body?.includes("## License agreement required") ||
        comment.body?.includes("## License agreement recorded")),
  );
}

function getRequiredEnv(name) {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required`);
  }

  return value;
}

function getRepoParts() {
  const repository = getRequiredEnv("GITHUB_REPOSITORY");
  const [owner, repo] = repository.split("/");

  if (!owner || !repo) {
    throw new Error(`Invalid GITHUB_REPOSITORY: ${repository}`);
  }

  return { owner, repo, repository };
}

async function githubRequest(path, options = {}) {
  const token = getRequiredEnv("GITHUB_TOKEN");
  const response = await fetch(`https://api.github.com${path}`, {
    ...options,
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      "x-github-api-version": "2022-11-28",
      ...options.headers,
    },
  });

  if (!response.ok) {
    const message = await response.text();
    const error = new Error(`GitHub API request failed: ${response.status} ${path} ${message}`);
    error.status = response.status;
    throw error;
  }

  if (response.status === 204) {
    return null;
  }

  return response.json();
}

async function listIssueComments({ owner, repo, issueNumber }) {
  const comments = [];

  for (let page = 1; ; page += 1) {
    const batch = await githubRequest(
      `/repos/${owner}/${repo}/issues/${issueNumber}/comments?per_page=100&page=${page}`,
    );
    comments.push(...batch);

    if (batch.length < 100) {
      return comments;
    }
  }
}

async function upsertStickyComment({ owner, repo, issueNumber, comments, body }) {
  const stickyComment = findStickyComment(comments);

  if (stickyComment) {
    await githubRequest(`/repos/${owner}/${repo}/issues/comments/${stickyComment.id}`, {
      method: "PATCH",
      body: JSON.stringify({ body }),
    });
    return;
  }

  await githubRequest(`/repos/${owner}/${repo}/issues/${issueNumber}/comments`, {
    method: "POST",
    body: JSON.stringify({ body }),
  });
}

async function createCommitStatus({ owner, repo, sha, state, description }) {
  const serverUrl = process.env.GITHUB_SERVER_URL || "https://github.com";
  const repository = getRequiredEnv("GITHUB_REPOSITORY");
  const runId = getRequiredEnv("GITHUB_RUN_ID");

  await githubRequest(`/repos/${owner}/${repo}/statuses/${sha}`, {
    method: "POST",
    body: JSON.stringify({
      context: STATUS_CONTEXT,
      description,
      state,
      target_url: `${serverUrl}/${repository}/actions/runs/${runId}`,
    }),
  });
}

async function fetchContributorsFile({ owner, repo }) {
  try {
    const result = await githubRequest(`/repos/${owner}/${repo}/contents/contributors.md`);
    const content = Buffer.from(result.content, "base64").toString("utf8");
    return { content, sha: result.sha };
  } catch (error) {
    if (error.status === 404) {
      return { content: "", sha: null };
    }
    throw error;
  }
}

async function addToContributorsFile({ owner, repo, username, content, sha }) {
  if (isInContributorsFile(username, content)) {
    return;
  }

  const newContent = content.trimEnd() + `\n- @${username}\n`;
  const encodedContent = Buffer.from(newContent).toString("base64");

  const body = {
    message: buildCommitMessage(username),
    content: encodedContent,
    committer: {
      name: "github-actions[bot]",
      email: "41898282+github-actions[bot]@users.noreply.github.com",
    },
  };

  if (sha) {
    body.sha = sha;
  }

  try {
    await githubRequest(`/repos/${owner}/${repo}/contents/contributors.md`, {
      method: "PUT",
      body: JSON.stringify(body),
    });
  } catch (error) {
    if (error.status === 409) {
      const { content: freshContent, sha: freshSha } = await fetchContributorsFile({ owner, repo });
      if (!isInContributorsFile(username, freshContent)) {
        const retryContent = freshContent.trimEnd() + `\n- @${username}\n`;
        body.content = Buffer.from(retryContent).toString("base64");
        body.sha = freshSha;
        await githubRequest(`/repos/${owner}/${repo}/contents/contributors.md`, {
          method: "PUT",
          body: JSON.stringify(body),
        });
      }
    } else {
      throw error;
    }
  }
}

async function getPullRequestForEvent({ event, owner, repo }) {
  if (event.pull_request) {
    return {
      issueNumber: event.pull_request.number,
      pullRequest: event.pull_request,
    };
  }

  if (!event.issue?.pull_request) {
    return null;
  }

  return {
    issueNumber: event.issue.number,
    pullRequest: await githubRequest(`/repos/${owner}/${repo}/pulls/${event.issue.number}`),
  };
}

export async function run() {
  const event = JSON.parse(await readFile(getRequiredEnv("GITHUB_EVENT_PATH"), "utf8"));
  const { owner, repo } = getRepoParts();
  const prContext = await getPullRequestForEvent({ event, owner, repo });

  if (!prContext) {
    console.log("No pull request found for this event; skipping license agreement check.");
    return;
  }

  const { issueNumber, pullRequest } = prContext;
  const prAuthor = pullRequest.user.login;
  const headSha = pullRequest.head.sha;
  const prBody = pullRequest.body || "";
  const { content: contributorsContent, sha: contributorsSha } = await fetchContributorsFile({
    owner,
    repo,
  });
  const inContributorsFile = isInContributorsFile(prAuthor, contributorsContent);

  if (inContributorsFile) {
    await createCommitStatus({
      owner,
      repo,
      sha: headSha,
      state: "success",
      description: "Contributor agreement on file.",
    });
    console.log(`${prAuthor} is already in contributors.md; license agreement satisfied.`);
    return;
  }

  if (hasCheckedCheckbox(prBody)) {
    await addToContributorsFile({
      owner,
      repo,
      username: prAuthor,
      content: contributorsContent,
      sha: contributorsSha,
    });

    const comments = await listIssueComments({ owner, repo, issueNumber });
    await upsertStickyComment({
      owner,
      repo,
      issueNumber,
      comments,
      body: buildSatisfiedComment(prAuthor),
    });
    await createCommitStatus({
      owner,
      repo,
      sha: headSha,
      state: "success",
      description: "License agreement recorded.",
    });
    console.log(`License agreement recorded for ${prAuthor}; added to contributors.md.`);
    return;
  }

  const comments = await listIssueComments({ owner, repo, issueNumber });
  await upsertStickyComment({
    owner,
    repo,
    issueNumber,
    comments,
    body: buildPendingComment(prAuthor),
  });
  await createCommitStatus({
    owner,
    repo,
    sha: headSha,
    state: "failure",
    description: "License agreement acknowledgement required.",
  });
  console.error(
    `::error::License agreement required. ${prAuthor} must check the agreement box in the PR description.`,
  );
  process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
