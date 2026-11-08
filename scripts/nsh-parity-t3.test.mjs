import test from 'node:test';
import assert from 'node:assert/strict';
import { issues, replacement } from './nsh-parity-t3.ts';

test('decoding issues are read with their paths', () => {
  const message = 'Missing key\n  at ["approvalPolicy"]\nExpected number, got ""\n  at ["thread"]["turns"][0]["createdAt"]\nunrelated';
  assert.deepEqual(issues(message), [
    { text: 'Missing key', path: ['approvalPolicy'] },
    { text: 'Expected number, got ""', path: ['thread', 'turns', 0, 'createdAt'] },
  ]);
});

test('what the schema expects fills what the fixture left out', () => {
  const named = {
    V2Status: { members: [{ fields: { type: { literal: 'idle' }, note: { ast: { _tag: 'String', context: { isOptional: true } } } } }] },
    V2Mode: { literals: ['untrusted', 'never'] },
  };
  assert.equal(replacement('Missing key', undefined, named), '');
  assert.equal(replacement('Expected number, got ""', '', named), 0);
  assert.equal(replacement('Expected boolean', '', named), false);
  assert.deepEqual(replacement('Expected array', '', named), []);
  assert.deepEqual(replacement('Expected object', '', named), {});
  assert.equal(replacement('Expected "cli" | "exec"', '', named), 'cli');
  assert.equal(replacement('Expected string, got ""', '', named), 'x');
  assert.equal(replacement('Expected V2Mode', '', named), 'untrusted');
  assert.deepEqual(replacement('Expected V2Status', '', named), { type: 'idle' });
  assert.equal(replacement('Something else', '', named), undefined);
});
