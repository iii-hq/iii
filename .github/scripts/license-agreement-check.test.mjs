import assert from 'node:assert/strict';
import test from 'node:test';

import {
  ACKNOWLEDGEMENT_PHRASE,
  STICKY_COMMENT_MARKER,
  evaluateAgreement,
  findStickyComment,
  hasAgreementComment,
  isAgreementComment,
  isTeamPermission,
  touchesEnginePaths,
} from './license-agreement-check.mjs';

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

test('touchesEnginePaths returns true for engine src changes', () => {
  assert.equal(touchesEnginePaths([{ filename: 'engine/src/foo.rs' }]), true);
});

test('touchesEnginePaths returns true for top-level engine files', () => {
  assert.equal(touchesEnginePaths([{ filename: 'engine/Cargo.toml' }]), true);
});

test('touchesEnginePaths returns false for non-engine changes', () => {
  assert.equal(
    touchesEnginePaths([{ filename: 'console/x.ts' }, { filename: 'docs/y.md' }]),
    false,
  );
});

test('touchesEnginePaths returns false for empty list', () => {
  assert.equal(touchesEnginePaths([]), false);
});

test('touchesEnginePaths accepts plain string entries', () => {
  assert.equal(touchesEnginePaths(['engine/src/foo.rs']), true);
  assert.equal(touchesEnginePaths(['console/foo.ts']), false);
});

test('touchesEnginePaths does not match unrelated paths that contain "engine"', () => {
  assert.equal(touchesEnginePaths([{ filename: 'docs/engine-overview.md' }]), false);
  assert.equal(touchesEnginePaths([{ filename: 'crates/engine-utils/src/lib.rs' }]), false);
});

test('passes any PR that does not touch engine code regardless of permission', () => {
  const result = evaluateAgreement({
    comments: [],
    permission: 'none',
    prAuthor: 'external-user',
    changedFiles: [{ filename: 'docs/intro.md' }],
  });

  assert.deepEqual(result, {
    acknowledged: true,
    engineTouched: false,
    commentAcknowledged: false,
    teamMember: false,
  });
});

test('passes team members touching engine code without an acknowledgement', () => {
  const result = evaluateAgreement({
    comments: [],
    permission: 'write',
    prAuthor: 'team-member',
    changedFiles: [{ filename: 'engine/src/lib.rs' }],
  });

  assert.deepEqual(result, {
    acknowledged: true,
    engineTouched: true,
    commentAcknowledged: false,
    teamMember: true,
  });
});

test('passes external contributors touching engine code with an acknowledgement comment', () => {
  const result = evaluateAgreement({
    comments: [{ body: ACKNOWLEDGEMENT_PHRASE, user: { login: 'external-user' } }],
    permission: 'read',
    prAuthor: 'external-user',
    changedFiles: [{ filename: 'engine/src/lib.rs' }],
  });

  assert.deepEqual(result, {
    acknowledged: true,
    engineTouched: true,
    commentAcknowledged: true,
    teamMember: false,
  });
});

test('fails external contributors touching engine code without acknowledgement', () => {
  const result = evaluateAgreement({
    comments: [],
    permission: 'none',
    prAuthor: 'external-user',
    changedFiles: [{ filename: 'engine/src/lib.rs' }],
  });

  assert.deepEqual(result, {
    acknowledged: false,
    engineTouched: true,
    commentAcknowledged: false,
    teamMember: false,
  });
});
