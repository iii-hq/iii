import assert from 'node:assert/strict';
import test from 'node:test';

import {
  ACKNOWLEDGEMENT_PHRASE,
  STICKY_COMMENT_MARKER,
  evaluateAgreement,
  findStickyComment,
  hasAgreementComment,
  hasCheckedLicenseAgreement,
  isAgreementComment,
  isTeamPermission,
} from './license-agreement-check.mjs';

test('detects a checked Apache license agreement in the PR body', () => {
  const body = [
    '## What',
    'A contribution',
    '',
    '- [x] I am licensing the entirety of this PR under Apache 2 and have all necessary rights to the code I am contributing.',
  ].join('\n');

  assert.equal(hasCheckedLicenseAgreement(body), true);
});

test('detects an uppercase checked Apache license agreement in the PR body', () => {
  const body = `- [X] ${ACKNOWLEDGEMENT_PHRASE}`;

  assert.equal(hasCheckedLicenseAgreement(body), true);
});

test('handles a null PR body', () => {
  assert.equal(hasCheckedLicenseAgreement(null), false);
});

test('rejects an unchecked Apache license agreement in the PR body', () => {
  const body =
    '- [ ] I am licensing the entirety of this PR under Apache 2 and have all necessary rights to the code I am contributing.';

  assert.equal(hasCheckedLicenseAgreement(body), false);
});

test('rejects checked tasks that do not mention licensing and Apache', () => {
  assert.equal(hasCheckedLicenseAgreement('- [x] Tests pass locally'), false);
});

test('accepts the exact acknowledgement phrase from the PR author', () => {
  const comment = {
    body: ACKNOWLEDGEMENT_PHRASE,
    user: { login: 'external-user' },
  };

  assert.equal(isAgreementComment(comment, 'external-user'), true);
});

test('rejects the acknowledgement phrase when embedded in a longer comment', () => {
  const comment = {
    body: `No, ${ACKNOWLEDGEMENT_PHRASE}`,
    user: { login: 'external-user' },
  };

  assert.equal(isAgreementComment(comment, 'external-user'), false);
});

test('ignores acknowledgement comments from other users', () => {
  const comment = {
    body: ACKNOWLEDGEMENT_PHRASE,
    user: { login: 'reviewer' },
  };

  assert.equal(isAgreementComment(comment, 'external-user'), false);
});

test('ignores bot-authored acknowledgement comments', () => {
  const comment = {
    body: ACKNOWLEDGEMENT_PHRASE,
    user: { login: 'github-actions[bot]' },
  };

  assert.equal(isAgreementComment(comment, 'github-actions[bot]'), false);
});

test('ignores the sticky comment marker as acknowledgement', () => {
  const comment = {
    body: `${STICKY_COMMENT_MARKER}\n${ACKNOWLEDGEMENT_PHRASE}`,
    user: { login: 'external-user' },
  };

  assert.equal(isAgreementComment(comment, 'external-user'), false);
});

test('finds an author acknowledgement among comments', () => {
  const comments = [
    { body: ACKNOWLEDGEMENT_PHRASE, user: { login: 'reviewer' } },
    { body: ACKNOWLEDGEMENT_PHRASE, user: { login: 'external-user' } },
  ];

  assert.equal(hasAgreementComment(comments, 'external-user'), true);
});

test('finds sticky comments only when authored by the workflow bot', () => {
  const comments = [
    { body: STICKY_COMMENT_MARKER, user: { login: 'external-user' } },
    { body: STICKY_COMMENT_MARKER, user: { login: 'github-actions[bot]' }, id: 123 },
  ];

  assert.deepEqual(findStickyComment(comments), comments[1]);
});

test('classifies write, maintain, and admin permissions as team members', () => {
  assert.equal(isTeamPermission('write'), true);
  assert.equal(isTeamPermission('maintain'), true);
  assert.equal(isTeamPermission('admin'), true);
});

test('classifies read, triage, and none permissions as external contributors', () => {
  assert.equal(isTeamPermission('read'), false);
  assert.equal(isTeamPermission('triage'), false);
  assert.equal(isTeamPermission('none'), false);
});

test('passes team members without an acknowledgement', () => {
  const result = evaluateAgreement({
    body: '',
    comments: [],
    permission: 'write',
    prAuthor: 'team-member',
  });

  assert.deepEqual(result, {
    acknowledged: true,
    bodyAcknowledged: false,
    commentAcknowledged: false,
    teamMember: true,
  });
});

test('passes external contributors with an acknowledgement comment', () => {
  const result = evaluateAgreement({
    body: '',
    comments: [{ body: ACKNOWLEDGEMENT_PHRASE, user: { login: 'external-user' } }],
    permission: 'read',
    prAuthor: 'external-user',
  });

  assert.deepEqual(result, {
    acknowledged: true,
    bodyAcknowledged: false,
    commentAcknowledged: true,
    teamMember: false,
  });
});

test('fails external contributors without body or comment acknowledgement', () => {
  const result = evaluateAgreement({
    body: '',
    comments: [],
    permission: 'none',
    prAuthor: 'external-user',
  });

  assert.deepEqual(result, {
    acknowledged: false,
    bodyAcknowledged: false,
    commentAcknowledged: false,
    teamMember: false,
  });
});
