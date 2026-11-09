import test from 'node:test';
import assert from 'node:assert/strict';
import {inspectVrmGlb, inspectVrmaGlb, validateVrmRecipe, VRM_DEFAULT_RECIPE,
  serializeVrmStudioProject, parseVrmStudioProject, VRM_LIMITS} from './vrm-model.mjs';

function glb(json, binary = new Uint8Array(64)) {
  const raw = new TextEncoder().encode(JSON.stringify(json));
  const jsonLength = Math.ceil(raw.length / 4) * 4, binLength = Math.ceil(binary.length / 4) * 4;
  const result = new Uint8Array(12 + 8 + jsonLength + 8 + binLength), view = new DataView(result.buffer);
  view.setUint32(0, 0x46546c67, true); view.setUint32(4, 2, true); view.setUint32(8, result.length, true);
  view.setUint32(12, jsonLength, true); view.setUint32(16, 0x4e4f534a, true); result.fill(32, 20, 20 + jsonLength); result.set(raw, 20);
  view.setUint32(20 + jsonLength, binLength, true); view.setUint32(24 + jsonLength, 0x004e4942, true); result.set(binary, 28 + jsonLength);
  return result.buffer;
}

function model(version = '1') {
  return {asset: {version: '2.0'}, buffers: [{byteLength: 64}], bufferViews: [{buffer: 0, byteOffset: 0, byteLength: 36}],
    accessors: [{bufferView: 0, componentType: 5126, count: 3, type: 'VEC3', min: [0, 0, 0], max: [1, 1, 1]}],
    meshes: [{primitives: [{attributes: {POSITION: 0}}]}], nodes: [{mesh: 0}], scenes: [{nodes: [0]}], scene: 0,
    extensionsUsed: [version === '1' ? 'VRMC_vrm' : 'VRM'],
    extensions: version === '1' ? {VRMC_vrm: {specVersion: '1.0', meta: {name: '合成角色', authors: ['Fixture']}, humanoid: {humanBones: {hips: {node: 0}}}}}
      : {VRM: {specVersion: '0.0', meta: {title: '合成旧版', author: 'Fixture'}, humanoid: {humanBones: [{bone: 'hips', node: 0}]}}}};
}

function motion() {
  const json = {asset: {version: '2.0'}, buffers: [{byteLength: 32}],
    bufferViews: [{buffer: 0, byteOffset: 0, byteLength: 8}, {buffer: 0, byteOffset: 8, byteLength: 24}],
    accessors: [{bufferView: 0, componentType: 5126, type: 'SCALAR', count: 2}, {bufferView: 1, componentType: 5126, type: 'VEC3', count: 2}],
    nodes: [{}], extensionsUsed: ['VRMC_vrm_animation'],
    extensions: {VRMC_vrm_animation: {specVersion: '1.0', humanoid: {humanBones: {hips: {node: 0}}}}},
    animations: [{samplers: [{input: 0, output: 1}], channels: [{sampler: 0, target: {node: 0, path: 'translation'}}]}]};
  const bytes = new Uint8Array(32); new DataView(bytes.buffer).setFloat32(4, 1, true);
  return {json, bytes};
}

function invalid(change, pattern = /./) { const json = model(); change(json); assert.throws(() => inspectVrmGlb(glb(json)), pattern); }

test('generated self-contained VRM 1 and 0 expose plain metadata and bounded geometry stats', () => {
  for (const version of ['1', '0']) {
    const input = glb(model(version)), info = inspectVrmGlb(input);
    assert.equal(info.version, version); assert.deepEqual(info.authors, ['Fixture']);
    assert.equal(info.stats.vertices, 3); assert.equal(info.stats.nodes, 1); assert.equal(info.stats.bytes, input.byteLength);
    assert.equal(typeof info.name, 'string'); assert.deepEqual(info.json, model(version));
  }
  assert.throws(() => inspectVrmaGlb(glb(model())), /VRMA/);
});

test('GLB rejects truncation, corrupt lengths, duplicate chunks, invalid UTF8/JSON and oversized admission', () => {
  const input = glb(model());
  for (const length of [0, 11, 20, input.byteLength - 1]) assert.throws(() => inspectVrmGlb(input.slice(0, length)));
  for (const [offset, value] of [[0, 0], [4, 1], [8, 12], [12, 0xfffffffc], [16, 0x004e4942]]) {
    const copy = input.slice(0); new DataView(copy).setUint32(offset, value, true); assert.throws(() => inspectVrmGlb(copy));
  }
  const corrupt = new Uint8Array(input.slice(0)); corrupt[20] = 0xff; assert.throws(() => inspectVrmGlb(corrupt));
  const duplicate = new Uint8Array(input.byteLength + 8); duplicate.set(new Uint8Array(input));
  const duplicateView = new DataView(duplicate.buffer); duplicateView.setUint32(8, duplicate.length, true);
  duplicateView.setUint32(input.byteLength + 4, 0x004e4942, true);
  assert.throws(() => inspectVrmGlb(duplicate), /数量/);
  const deep = model(); let cursor = deep.extras = {};
  for (let i = 0; i < 65; i++) cursor = cursor.child = {};
  assert.throws(() => inspectVrmGlb(glb(deep)), /深度/);
  assert.throws(() => inspectVrmGlb(new Uint8Array(VRM_LIMITS.modelBytes + 1)), /大小|上限/);
});

test('external resource URIs and every unsupported codec path fail before a loader can fetch or decode', () => {
  for (const uri of ['https://example.org/a.bin', 'data:application/octet-stream;base64,AAAA', '', '../x']) invalid(j => {j.buffers[0].uri = uri;}, /URI/);
  invalid(j => {j.images = [{uri: 'https://example.org/a.png'}];}, /URI/);
  for (const name of ['KHR_draco_mesh_compression', 'EXT_meshopt_compression', 'KHR_texture_basisu', 'EXT_texture_webp']) {
    invalid(j => {j.meshes[0].primitives[0].extensions = {[name]: {}};}, /不支持/);
  }
  invalid(j => {j.extensionsRequired = ['not_a_supported_extension'];}, /不支持/);
});

test('all byte views/accessors, matrix padding, implicit zero data and sparse indices are bounded', () => {
  invalid(j => {j.bufferViews[0].byteOffset = 60;}, /范围/);
  invalid(j => {j.bufferViews[0].buffer = 1;}, /引用/);
  invalid(j => {j.accessors[0].count = 100;}, /范围/);
  invalid(j => {j.accessors[0].bufferView = 3;}, /引用/);
  invalid(j => {j.accessors[0].byteOffset = 1;}, /对齐/);
  invalid(j => {j.accessors[0].componentType = '5126';}, /不支持/);
  invalid(j => {j.accessors[0].type = 'constructor';}, /不支持/);
  invalid(j => {j.accessors[0].sparse = {count: 4, indices: {bufferView: 0, componentType: 5121}, values: {bufferView: 0}};}, /sparse/);
  invalid(j => {j.accessors[0] = {componentType: 5126, type: 'VEC3', count: 200001};}, /顶点/);
  const j = model(); j.accessors.push({componentType: 5121, type: 'MAT3', count: 6, bufferView: 0});
  assert.throws(() => inspectVrmGlb(glb(j)), /范围/);
  const sparse = model(); sparse.bufferViews.push({buffer: 0, byteOffset: 36, byteLength: 1}, {buffer: 0, byteOffset: 40, byteLength: 12});
  sparse.accessors[0].sparse = {count: 1, indices: {bufferView: 1, componentType: 5121}, values: {bufferView: 2}};
  const bytes = new Uint8Array(64); bytes[36] = 3;
  assert.throws(() => inspectVrmGlb(glb(sparse, bytes)), /sparse/);
  bytes[36] = 2; assert.equal(inspectVrmGlb(glb(sparse, bytes)).stats.vertices, 3);
  invalid(j => {j.accessors.push({componentType: 5126, type: 'MAT4', count: 1100000});}, /预算/);
});

test('node cycles, duplicate parents, scene/skin/mesh/material references and geometry budgets are rejected', () => {
  invalid(j => {j.nodes[0].children = [0];}, /循环/);
  invalid(j => {j.nodes.push({children: [0]}, {children: [0]});}, /父/);
  invalid(j => {j.scenes[0].nodes = [99];}, /引用/);
  invalid(j => {j.nodes[0].mesh = 99;}, /引用/);
  invalid(j => {j.nodes[0].skin = 0; j.skins = [{joints: [99]}];}, /引用/);
  invalid(j => {j.meshes[0].primitives[0].material = 0;}, /引用/);
  invalid(j => {j.nodes = Array.from({length: 513}, () => ({}));}, /nodes/);
  invalid(j => {j.materials = Array.from({length: 129}, () => ({}));}, /materials/);
  invalid(j => {j.extensions.VRMC_vrm.humanoid.humanBones.hips.node = 2;}, /引用/);
  invalid(j => {j.accessors[0] = {componentType: 5126, type: 'VEC3', count: 100000}; j.nodes.push({mesh: 0}, {mesh: 0});}, /顶点/);
  const indexed = model(); indexed.bufferViews.push({buffer: 0, byteOffset: 36, byteLength: 1});
  indexed.accessors.push({bufferView: 1, componentType: 5121, type: 'SCALAR', count: 1}); indexed.meshes[0].primitives[0].indices = 1;
  const badIndex = new Uint8Array(64); badIndex[36] = 3;
  assert.throws(() => inspectVrmGlb(glb(indexed, badIndex)), /顶点范围/);
});

test('draw admission counts shared indices and every mesh node instance rather than unique vertices', () => {
  const shared = model(); shared.accessors.push({componentType: 5121, type: 'SCALAR', count: 2000000});
  shared.meshes[0].primitives = Array.from({length: 512}, () => ({attributes: {POSITION: 0}, indices: 1}));
  assert.throws(() => inspectVrmGlb(glb(shared)), /绘制/);
  shared.accessors[1].count = 3000;
  assert.throws(() => inspectVrmGlb(glb(shared)), /绘制/);
  shared.meshes[0].primitives = [{attributes: {POSITION: 0}, indices: 1}];
  shared.accessors[1].count = 600000; shared.nodes.push({mesh: 0});
  assert.throws(() => inspectVrmGlb(glb(shared)), /绘制/);
  shared.accessors[1].count = 400000;
  assert.equal(inspectVrmGlb(glb(shared)).stats.drawElements, 800000);
});

test('morph admission counts expanded float textures, attribute slots and repeated geometry instances', () => {
  const j = model(); j.accessors[0] = {componentType: 5126, type: 'VEC3', count: 200000};
  j.meshes[0].primitives[0].targets = Array.from({length: 128}, () => ({POSITION: 0}));
  assert.throws(() => inspectVrmGlb(glb(j)), /morph.*预算/);
  j.accessors[0].count = 20000;
  j.meshes[0].primitives[0].targets = Array.from({length: 64}, () => ({POSITION: 0}));
  const one = inspectVrmGlb(glb(j)).stats.morphBytes;
  assert.ok(one > 0 && one <= VRM_LIMITS.morphBytes);
  j.nodes.push({mesh: 0});
  assert.throws(() => inspectVrmGlb(glb(j)), /morph.*预算/);
  j.nodes.pop(); j.meshes[0].primitives[0].targets = Array.from({length: 64}, () => ({POSITION: 0, NORMAL: 0}));
  assert.throws(() => inspectVrmGlb(glb(j)), /morph.*预算/);
  j.meshes[0].primitives[0].targets = Array.from({length: 64}, () => ({POSITION: 0, COLOR_0: 0}));
  assert.throws(() => inspectVrmGlb(glb(j)), /morph.*预算/);
});

function png(width, height) {
  const bytes = new Uint8Array(24), view = new DataView(bytes.buffer);
  view.setUint32(0, 0x89504e47); view.setUint32(4, 0x0d0a1a0a); view.setUint32(8, 13); view.setUint32(12, 0x49484452);
  view.setUint32(16, width); view.setUint32(20, height); return bytes;
}

test('embedded PNG/JPEG dimensions are checked before decoder allocation, including aggregate pixels', () => {
  const j = model(); j.bufferViews.push({buffer: 0, byteOffset: 36, byteLength: 24});
  j.images = [{bufferView: 1, mimeType: 'image/png'}]; j.textures = [{source: 0}];
  let bytes = new Uint8Array(64); bytes.set(png(128, 128), 36);
  assert.equal(inspectVrmGlb(glb(j, bytes)).stats.texturePixels, 16384);
  bytes.set(png(8192, 8192), 36); assert.throws(() => inspectVrmGlb(glb(j, bytes)), /像素/);
  bytes.set(png(128, 128), 36); j.images[0].mimeType = 'image/jpeg'; assert.throws(() => inspectVrmGlb(glb(j, bytes)), /JPEG/);
  j.images[0].mimeType = 'image/png'; j.images.push({...j.images[0]}); bytes.set(png(4096, 4097), 36);
  assert.throws(() => inspectVrmGlb(glb(j, bytes)), /像素/);
  j.images.pop(); j.textures = [{source: 0}, {source: 0}];
  assert.throws(() => inspectVrmGlb(glb(j, bytes)), /纹理实例/);
  j.textures.pop(); j.images[0].mimeType = 'image/jpeg'; j.bufferViews[1].byteLength = 13;
  bytes.set([0xff, 0xd8, 0xff, 0xc0, 0, 8, 8, 0, 128, 0, 64, 0, 0], 36);
  assert.equal(inspectVrmGlb(glb(j, bytes)).stats.texturePixels, 64 * 128);
});

test('VRMA channels validate sampler refs, counts, paths and strictly increasing finite times', () => {
  const {json, bytes} = motion(); const info = inspectVrmaGlb(glb(json, bytes));
  assert.equal(info.version, '1'); assert.equal(info.stats.durationSeconds, 1); assert.equal(info.stats.channels, 1);
  for (const change of [j => {j.animations[0].channels[0].sampler = 4;}, j => {j.animations[0].channels[0].target.node = 9;},
    j => {j.animations[0].channels[0].target.path = 'pointer';}, j => {j.accessors[1].count = 1;}]) {
    const copy = structuredClone(json); change(copy); assert.throws(() => inspectVrmaGlb(glb(copy, bytes)));
  }
  for (const value of [0, -1, NaN, Infinity]) {
    const bad = bytes.slice(); new DataView(bad.buffer).setFloat32(4, value, true);
    assert.throws(() => inspectVrmaGlb(glb(json, bad)), /时间|有限/);
  }
});

test('recipe defaults preserve fixed 40ms frame counts and reject unknown or coercible values', () => {
  assert.deepEqual(validateVrmRecipe(), VRM_DEFAULT_RECIPE);
  assert.equal(validateVrmRecipe({durationTicks: 250}).durationTicks * 40, 10000);
  const input = {preset: 'dance', yaw: 90, durationTicks: 40}; const copy = structuredClone(input);
  assert.equal(validateVrmRecipe(input).durationTicks, 40); assert.deepEqual(input, copy);
  for (const bad of [{durationTicks: 0}, {durationTicks: 251}, {durationTicks: '40'}, {yaw: Infinity}, {pitch: 61}, {zoom: 0},
    {targetY: -1}, {preset: 'missing'}, {loop: 1}, {inPlace: 'true'}, {spring: null}, {expressionWeight: 1.1}, {source: 'url'}]) assert.throws(() => validateVrmRecipe(bad));
});

test('portable studio JSON roundtrips original bytes, filenames, optional motion and normalized recipe', () => {
  const modelBuffer = glb(model()), m = motion(), motionBuffer = glb(m.json, m.bytes);
  const source = {modelBuffer, modelFileName: 'avatar.vrm', motionBuffer, motionFileName: 'dance.vrma', recipe: {preset: 'vrma', durationTicks: 40}, modelHash: 'a'.repeat(64)};
  const json = serializeVrmStudioProject(source), restored = parseVrmStudioProject(json);
  assert.deepEqual(new Uint8Array(restored.modelBuffer), new Uint8Array(modelBuffer));
  assert.deepEqual(new Uint8Array(restored.motionBuffer), new Uint8Array(motionBuffer));
  assert.equal(restored.modelFileName, 'avatar.vrm'); assert.equal(restored.motionFileName, 'dance.vrma');
  assert.equal(restored.recipe.preset, 'vrma'); assert.equal(restored.modelHash, source.modelHash);
  assert.equal(restored.modelInfo.name, '合成角色'); assert.equal(restored.motionInfo.stats.channels, 1);
  const modelOnly = parseVrmStudioProject(serializeVrmStudioProject({modelBuffer, modelFileName: 'a.vrm'}));
  assert.equal(modelOnly.motionBuffer, null);
  assert.equal(parseVrmStudioProject(serializeVrmStudioProject({...modelOnly, motionHash: null})).motionBuffer, null);
});

test('studio sources reject version drift, invalid base64, source corruption, unsafe filenames and motion-less VRMA recipes', () => {
  const json = JSON.parse(serializeVrmStudioProject({modelBuffer: glb(model()), modelFileName: 'a.vrm'}));
  for (const change of [j => {j.version = 2;}, j => {j.format = 'other';}, j => {j.modelBase64 += '!';}, j => {j.modelBase64 = 'AAAA';},
    j => {j.modelFileName = '../x.vrm';}, j => {j.modelHash = 'abc';}, j => {j.recipe.preset = 'vrma';}, j => {j.unknown = true;}]) {
    const copy = structuredClone(json); change(copy); assert.throws(() => parseVrmStudioProject(JSON.stringify(copy)));
  }
  assert.throws(() => parseVrmStudioProject(' '.repeat(VRM_LIMITS.studioBytes + 1)), /大小|上限/);
});

test('multi-megabyte embedded sources roundtrip without regex recursion or argument-stack overflow', () => {
  const j = model(), bytes = new Uint8Array(2 * 1024 * 1024); j.buffers[0].byteLength = bytes.length;
  const buffer = glb(j, bytes), encoded = serializeVrmStudioProject({modelBuffer: buffer});
  assert.equal(parseVrmStudioProject(encoded).modelBuffer.byteLength, buffer.byteLength);
});
