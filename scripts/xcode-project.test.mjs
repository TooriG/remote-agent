import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';

test('the iPhone app compiles every Swift source once with unique Xcode objects', () => {
  const project = readFileSync(new URL('../apps/mobile/iosApp/Bex.xcodeproj/project.pbxproj', import.meta.url), 'utf8');
  const swiftSources = readdirSync(new URL('../apps/mobile/iosApp/Bex/', import.meta.url), { recursive: true })
    .filter(path => path.endsWith('.swift')).map(path => path.split(/[\\/]/).at(-1));
  const definitions = [...project.matchAll(/^[ \t]*([A-F0-9]{24})(?: \/\*[^\n]*\*\/)? = \{[ \t]*(?:\r?\n[ \t]*)?(isa = [\s\S]*?)\};/gm)];
  const objects = new Map(definitions.map(([, id, body]) => [id, body]));
  assert.equal(objects.size, definitions.length, 'Xcode object identifiers must be unique');
  const target = [...objects.values()].find(body => body.includes('isa = PBXNativeTarget;') && /(?:^|\n)\s*name = Bex;/.test(body));
  assert(target, 'The Bex app target must be present');
  const phases = target.match(/\bbuildPhases = \(([\s\S]*?)\);/)?.[1];
  assert(phases, 'The Bex app must declare build phases');
  const sources = [...phases.matchAll(/[A-F0-9]{24}/g)].map(([id]) => objects.get(id)).find(body => body?.includes('isa = PBXSourcesBuildPhase;'));
  assert(sources, 'The Bex app must have a Sources build phase');
  const files = sources.match(/\bfiles = \(([\s\S]*?)\);/)?.[1];
  assert(files, 'The Sources build phase must declare files');
  const compiled = new Map();
  for (const [id] of files.matchAll(/[A-F0-9]{24}/g)) {
    const build = objects.get(id);
    assert(build?.includes('isa = PBXBuildFile;'), `Missing build file ${id}`);
    const reference = build.match(/\bfileRef = ([A-F0-9]{24})/)?.[1];
    const file = objects.get(reference);
    assert(file?.includes('isa = PBXFileReference;'), `Missing file reference ${reference}`);
    const path = file.match(/\bpath = ("[^"]*"|[^;]+);/)?.[1]?.replace(/^"|"$/g, '');
    assert(path, `Missing source path for ${reference}`);
    const name = path.split('/').at(-1);
    compiled.set(name, (compiled.get(name) ?? 0) + 1);
  }
  for (const name of swiftSources) {
    assert.equal(compiled.get(name), 1, `${name} must be compiled exactly once by the Bex app`);
  }
});
