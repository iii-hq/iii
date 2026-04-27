import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export const AGREEMENT_TEXT =
  'I am licensing the entirety of this PR under Apache 2 and have all necessary rights to the code I am contributing.';
export const ACKNOWLEDGEMENT_PHRASE = AGREEMENT_TEXT;
export const STATUS_CONTEXT = 'license-agreement';
export const STICKY_COMMENT_MARKER = '<!-- iii-license-agreement-check -->';

const TEAM_PERMISSIONS = new Set(['write', 'maintain', 'admin']);
const BOT_LOGINS = new Set(['github-actions[bot]']);

export function hasCheckedLicenseAgreement(body = '') {
  return String(body ?? '')
    .split(/\r?\n/)
    .some((line) => {
      const match = line.match(/^\s*-\s*\[[xX]\]\s*(.*)$/);
      return match && normalizeAgreementText(match[1]) === normalizeAgreementText(AGREEMENT_TEXT);
    });
}

export function normalizeAgreementText(value = '') {
  return value.replace(/\s+/g, ' ').trim().toLowerCase();
}

export function isTeamPermission(permission = '') {
  return TEAM_PERMISSIONS.has(permission);
}

export function isAgreementComment(comment, prAuthor) {
  if (!comment?.body || !comment?.user?.login) {
    return false;
  }

  if (BOT_LOGINS.has(comment.user.login)) {
    return false;
  }

  if (comment.user.login !== prAuthor) {
    return false;
  }

  if (comment.body.includes(STICKY_COMMENT_MARKER)) {
    return false;
  }

  return normalizeAgreementText(comment.body) === normalizeAgreementText(ACKNOWLEDGEMENT_PHRASE);
}

export function hasAgreementComment(comments = [], prAuthor) {
  return comments.some((comment) => isAgreementComment(comment, prAuthor));
}

export function findStickyComment(comments = []) {
  return comments.find(
    (comment) =>
      comment.body?.includes(STICKY_COMMENT_MARKER) && BOT_LOGINS.has(comment.user?.login),
  );
}

export function evaluateAgreement({ body = '', comments = [], permission = '', prAuthor = '' }) {
  const teamMember = isTeamPermission(permission);
  const bodyAcknowledged = hasCheckedLicenseAgreement(body);
  const commentAcknowledged = hasAgreementComment(comments, prAuthor);
  const acknowledged = teamMember || bodyAcknowledged || commentAcknowledged;

  return {
    acknowledged,
    bodyAcknowledged,
    commentAcknowledged,
    teamMember,
  };
}

export function buildPendingComment(prAuthor) {
  return [
    STICKY_COMMENT_MARKER,
    '## License agreement required',
    '',
    `@${prAuthor}, thanks for contributing. Before this PR can be merged, please acknowledge the contributor license agreement:`,
    '',
    `> ${AGREEMENT_TEXT}`,
    '',
    'You can satisfy this requirement in either of these ways:',
    '',
    `- Check the license agreement box in the PR description.`,
    `- Reply to this PR with exactly: ${ACKNOWLEDGEMENT_PHRASE}`,
  ].join('\n');
}

export function buildSatisfiedComment(prAuthor, source) {
  const sourceLabel = source === 'comment' ? 'PR comment' : 'PR description checkbox';

  return [
    STICKY_COMMENT_MARKER,
    '## License agreement recorded',
    '',
    `@${prAuthor}, the license agreement acknowledgement has been recorded from the ${sourceLabel}.`,
    '',
    `> ${AGREEMENT_TEXT}`,
  ].join('\n');
}

function getRequiredEnv(name) {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required`);
  }

  return value;
}

function getRepoParts() {
  const repository = getRequiredEnv('GITHUB_REPOSITORY');
  const [owner, repo] = repository.split('/');

  if (!owner || !repo) {
    throw new Error(`Invalid GITHUB_REPOSITORY: ${repository}`);
  }

  return { owner, repo, repository };
}

async function githubRequest(path, options = {}) {
  const token = getRequiredEnv('GITHUB_TOKEN');
  const response = await fetch(`https://api.github.com${path}`, {
    ...options,
    headers: {
      accept: 'application/vnd.github+json',
      authorization: `Bearer ${token}`,
      'content-type': 'application/json',
      'x-github-api-version': '2022-11-28',
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

async function getPermission({ owner, repo, username }) {
  try {
    const result = await githubRequest(
      `/repos/${owner}/${repo}/collaborators/${encodeURIComponent(username)}/permission`,
    );

    return result.permission || 'none';
  } catch (error) {
    if (error.status === 404) {
      return 'none';
    }

    throw error;
  }
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
      method: 'PATCH',
      body: JSON.stringify({ body }),
    });
    return;
  }

  await githubRequest(`/repos/${owner}/${repo}/issues/${issueNumber}/comments`, {
    method: 'POST',
    body: JSON.stringify({ body }),
  });
}

async function createCommitStatus({ owner, repo, sha, state, description }) {
  const serverUrl = process.env.GITHUB_SERVER_URL || 'https://github.com';
  const repository = getRequiredEnv('GITHUB_REPOSITORY');
  const runId = getRequiredEnv('GITHUB_RUN_ID');

  await githubRequest(`/repos/${owner}/${repo}/statuses/${sha}`, {
    method: 'POST',
    body: JSON.stringify({
      context: STATUS_CONTEXT,
      description,
      state,
      target_url: `${serverUrl}/${repository}/actions/runs/${runId}`,
    }),
  });
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
  const event = JSON.parse(await readFile(getRequiredEnv('GITHUB_EVENT_PATH'), 'utf8'));
  const { owner, repo } = getRepoParts();
  const prContext = await getPullRequestForEvent({ event, owner, repo });

  if (!prContext) {
    console.log('No pull request found for this event; skipping license agreement check.');
    return;
  }

  const { issueNumber, pullRequest } = prContext;
  const prAuthor = pullRequest.user.login;
  const headSha = pullRequest.head.sha;
  const comments = await listIssueComments({ owner, repo, issueNumber });
  const permission = await getPermission({ owner, repo, username: prAuthor });
  const result = evaluateAgreement({
    body: pullRequest.body || '',
    comments,
    permission,
    prAuthor,
  });

  if (result.teamMember) {
    await createCommitStatus({
      owner,
      repo,
      sha: headSha,
      state: 'success',
      description: 'iii team member; license agreement check skipped.',
    });
    console.log(`Skipping license agreement prompt for ${prAuthor} with ${permission} permission.`);
    return;
  }

  if (result.acknowledged) {
    const source = result.commentAcknowledged ? 'comment' : 'body';
    await upsertStickyComment({
      owner,
      repo,
      issueNumber,
      comments,
      body: buildSatisfiedComment(prAuthor, source),
    });
    await createCommitStatus({
      owner,
      repo,
      sha: headSha,
      state: 'success',
      description: 'License agreement acknowledged.',
    });
    console.log(`License agreement acknowledged by ${prAuthor} through ${source}.`);
    return;
  }

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
    state: 'failure',
    description: 'License agreement acknowledgement required.',
  });
  console.error(
    `::error::License agreement acknowledgement required. ${prAuthor} must check the PR description box or reply with the exact acknowledgement phrase.`,
  );
  process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
