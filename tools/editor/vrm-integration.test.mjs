import test from 'node:test';
import assert from 'node:assert/strict';
import {prepareVrmAction} from './vrm-integration.mjs';
import {VRM_DEFAULT_RECIPE} from './vrm-model.mjs';

const src = 'data:image/png;base64,cG5n';
function fixture() {
  const project = {assets:[{id:'old',src}],actions:[{id:'idle'}],defaultActionId:'idle',rules:[{id:'rule'}]};
  const payload = {projectToken:project,name:'新动作',loop:true,assets:[{name:'1.png',src},{name:'2.png',src:src+'Mg=='}],
    source3d:{modelName:'sample.vrm',modelHash:'abc',recipe:{...VRM_DEFAULT_RECIPE,durationTicks:2},modelBuffer:'NEVER COPY'}};
  let id = 0;
  return {project,payload,prepare:() => prepareVrmAction(project,payload,prefix => `${prefix}-${++id}`)};
}
test('bake appends fixed ticks with dedup and keeps source bytes out of desktop data', () => {
  const f = fixture(), original = JSON.stringify(f.project), prepared = f.prepare();
  assert.equal(prepared.assets.length,1);
  assert.equal(prepared.action.frames[0].assetId,'old');
  assert.equal(prepared.action.frames[1].assetId,prepared.assets[0].id);
  assert.deepEqual(prepared.action.frames.map(x => [x.ticks,x.x,x.y]),[[1,0,0],[1,0,0]]);
  assert.equal(prepared.action.source3d.modelName,'sample.vrm');
  assert.equal('modelBuffer' in prepared.action.source3d,false);
  assert.equal(JSON.stringify(f.project),original);
});
test('changed projects and invalid frame counts cannot partially append', () => {
  const f = fixture(); f.payload.projectToken = structuredClone(f.project);
  assert.throws(f.prepare,/工程已变化/);
  f.payload.projectToken = f.project; f.payload.assets.pop();
  assert.throws(f.prepare,/帧数/); assert.equal(f.project.actions.length,1);
});
test('asset and action capacity rejects whole bake', () => {
  const f = fixture(); f.project.assets = Array.from({length:256},(_,i) => ({id:`a${i}`,src:`x${i}`}));
  assert.throws(f.prepare,/256/);
  f.project.actions = Array.from({length:64},() => ({})); assert.throws(f.prepare,/64/);
});
