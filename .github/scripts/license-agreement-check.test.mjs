import assert from 'node:assert/strict';
import test from 'node:test';

import {
  CHECKBOX_TEXT,
  buildCommitMessage,
  buildPendingComment,
  buildSatisfiedComment,
  evaluateAgreement,
  findStickyComment,
  hasCheckedCheckbox,
  isInContributorsFile,
} from './license-agreement-check.mjs';

// ── hasCheckedCheckbox ────────────────────────────────────────────────────────

test('detects a checked checkbox in the PR body', () => {
  assert.equal(hasCheckedCheckbox(`- [x] ${CHECKBOX_TEXT}`), true);
});

test('detects a checked checkbox regardless of case', () => {
  assert.equal(hasCheckedCheckbox(`- [X] ${CHECKBOX_TEXT}`), true);
});

test('does not treat an unchecked checkbox as checked', () => {
  assert.equal(hasCheckedCheckbox(`- [ ] ${CHECKBOX_TEXT}`), false);
});

test('returns false when the PR body is empty', () => {
  assert.equal(hasCheckedCheckbox(''), false);
});

// ── isInContributorsFile ──────────────────────────────────────────────────────

test('finds a username that appears as a list entry', () => {
  const content = '# Contributors\n\n- @alice\n- @bob\n';
  assert.equal(isInContributorsFile('alice', content), true);
  assert.equal(isInContributorsFile('bob', content), true);
});

test('does not match a partial username', () => {
  const content = '# Contributors\n\n- @alice-bot\n';
  assert.equal(isInContributorsFile('alice', content), false);
});

test('returns false when the file is empty', () => {
  assert.equal(isInContributorsFile('alice', ''), false);
});

test('returns false when the username is not present', () => {
  assert.equal(isInContributorsFile('carol', '# Contributors\n\n- @alice\n'), false);
});

// ── evaluateAgreement ─────────────────────────────────────────────────────────

test('passes contributors already in contributors.md', () => {
  const result = evaluateAgreement({ prBody: '', inContributorsFile: true });

  assert.deepEqual(result, {
    acknowledged: true,
    checkboxChecked: false,
    inContributorsFile: true,
  });
});

test('passes contributors who have checked the checkbox', () => {
  const result = evaluateAgreement({ prBody: `- [x] ${CHECKBOX_TEXT}`, inContributorsFile: false });

  assert.deepEqual(result, {
    acknowledged: true,
    checkboxChecked: true,
    inContributorsFile: false,
  });
});

test('fails contributors without acknowledgement', () => {
  const result = evaluateAgreement({ prBody: '', inContributorsFile: false });

  assert.deepEqual(result, {
    acknowledged: false,
    checkboxChecked: false,
    inContributorsFile: false,
  });
});

// ── findStickyComment ─────────────────────────────────────────────────────────

test('finds the pending comment when authored by the workflow bot', () => {
  const comments = [
    { body: '## License agreement required', user: { login: 'external-user' } },
    { body: '## License agreement required', user: { login: 'github-actions[bot]' }, id: 123 },
  ];

  assert.deepEqual(findStickyComment(comments), comments[1]);
});

test('finds the satisfied comment when authored by the workflow bot', () => {
  const comments = [
    { body: '## License agreement recorded', user: { login: 'github-actions[bot]' }, id: 456 },
  ];

  assert.deepEqual(findStickyComment(comments), comments[0]);
});

test('returns undefined when no bot comment exists', () => {
  const comments = [
    { body: '## License agreement required', user: { login: 'external-user' } },
  ];

  assert.equal(findStickyComment(comments), undefined);
});

// ── buildCommitMessage ────────────────────────────────────────────────────────

test('buildCommitMessage includes the username', () => {
  const msg = buildCommitMessage('alice');
  assert.ok(msg.includes('@alice'));
  assert.ok(msg.startsWith('docs:'));
});

// ── buildPendingComment ───────────────────────────────────────────────────────

test('buildPendingComment includes the checkbox text as a copyable code sample', () => {
  const comment = buildPendingComment('alice');
  assert.ok(comment.includes('@alice'));
  assert.ok(comment.includes('docs: add @alice to contributors.md'));
  assert.ok(comment.includes('```markdown'));
  assert.ok(comment.includes(`- [ ] ${CHECKBOX_TEXT}`));
});

// ── buildSatisfiedComment ─────────────────────────────────────────────────────

test('buildSatisfiedComment includes the username and contributors.md reference', () => {
  const comment = buildSatisfiedComment('alice');
  assert.ok(comment.includes('@alice'));
  assert.ok(comment.includes('contributors.md'));
});
