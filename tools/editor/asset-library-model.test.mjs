import test from 'node:test';
import assert from 'node:assert/strict';
import {assetReferences, deriveAssetGroups, deleteUnusedAssets, renameAssetGroup, createAssetGroup, removeAssetGroup, validateAssetGroups} from './asset-library-model.mjs';

const fixture = () => ({assets:[{id:'a',name:'内置',src:'/assets/shime1.png'},{id:'b',name:'重复动画',src:'data:b'},{id:'c',name:'部件',src:'data:c'},{id:'d',name:'关键帧',src:'data:d'},{id:'e',name:'未用',src:'data:e'}],parts:[{id:'part',assetId:'c'}],actions:[{id:'walk',name:'走路',frames:[{assetId:'a',ticks:2},{assetId:'b',ticks:3},{assetId:'b',ticks:4}]},{id:'rig',name:'部件动作',kind:'parts',frames:[{assetId:'a',ticks:1}],tracks:[{partId:'part',keys:[{tick:0,assetId:'c'},{tick:1,assetId:'d'}]}]}]});

test('usage includes cross-action frames, part bases and asset keys but excludes group membership',()=>{
  const p=fixture();p.assetGroups=[{id:'source',name:'来源',frames:[{assetId:'e',ticks:1}]}];
  const refs=assetReferences(p);
  assert.equal(refs.get('a').count,2);assert.equal(refs.get('b').frames,2);
  assert.equal(refs.get('c').parts,1);assert.equal(refs.get('c').keys,1);
  assert.equal(refs.get('d').keys,1);assert.equal(refs.get('e').count,0);
  assert.ok(refs.get('c').actionIds.includes('rig'));
});
test('legacy derivation is stable, immutable and preserves repeated animation frames',()=>{
  const p=fixture(),before=JSON.stringify(p),groups=deriveAssetGroups(p);
  assert.deepEqual(groups.map(g=>g.id),deriveAssetGroups(p).map(g=>g.id));
  assert.deepEqual(groups.find(g=>g.id==='virtual:action:walk').frames,[{assetId:'b',ticks:3},{assetId:'b',ticks:4}]);
  assert.equal(groups.find(g=>g.id==='virtual:builtin').frames[0].assetId,'a');
  assert.equal(JSON.stringify(p),before);
});
test('explicit overlapping groups retain source order without redundant virtual membership',()=>{
  const p=fixture();p.assetGroups=[{id:'first',name:'第一组',frames:[{assetId:'b',ticks:1},{assetId:'b',ticks:7}]},{id:'second',name:'第二组',frames:[{assetId:'b',ticks:2}]}];
  const groups=deriveAssetGroups(p);
  assert.deepEqual(groups[0].frames,p.assetGroups[0].frames);assert.equal(groups[1].frames[0].assetId,'b');
  assert.equal(groups.filter(g=>g.virtual).some(g=>g.frames.some(f=>f.assetId==='b')),false);
});
test('deletion rejects a partially used batch atomically, protects key-only references, repairs group refs',()=>{
  const p=fixture();p.assetGroups=[{id:'mixed',name:'混合',frames:[{assetId:'e',ticks:1},{assetId:'b',ticks:2}]},{id:'empty',name:'将清空',frames:[{assetId:'e',ticks:1}]}];
  const before=JSON.stringify(p);
  assert.throws(()=>deleteUnusedAssets(p,['e','d']),/使用/);assert.equal(JSON.stringify(p),before);
  deleteUnusedAssets(p,['e']);assert.equal(p.assets.some(a=>a.id==='e'),false);
  assert.deepEqual(p.assetGroups,[{id:'mixed',name:'混合',frames:[{assetId:'b',ticks:2}]}]);
});
test('renaming a virtual group upgrades only that metadata and regrouping preserves existing membership',()=>{
  const p=fixture();renameAssetGroup(p,'virtual:action:walk','新名字',()=> 'upgraded');
  assert.deepEqual(p.assetGroups[0],{id:'upgraded',name:'新名字',frames:[{assetId:'b',ticks:3},{assetId:'b',ticks:4}]});
  createAssetGroup(p,['b','e','b'],'整理',()=> 'grouped');assert.equal(p.assetGroups.length,2);
  assert.deepEqual(p.assetGroups[1].frames,[{assetId:'b',ticks:6},{assetId:'e',ticks:6}]);
  removeAssetGroup(p,'grouped');assert.equal(p.assets.length,5);assert.equal(p.assetGroups.length,1);
});
test('invalid group names, unknown refs and invalid timing fail before mutation',()=>{
  const p=fixture(),before=JSON.stringify(p);
  assert.throws(()=>createAssetGroup(p,['missing'],'组',()=> 'id'));assert.throws(()=>createAssetGroup(p,['b'],' ',()=> 'id'));
  assert.equal(JSON.stringify(p),before);
  assert.throws(()=>validateAssetGroups([{id:'x',name:'组',frames:[{assetId:'b',ticks:0}]}],p.assets));
  assert.throws(()=>validateAssetGroups([{id:'x',name:'组',frames:[{assetId:'b',ticks:1}]},{id:'x',name:'重复',frames:[{assetId:'b',ticks:1}]}],p.assets));
  assert.doesNotThrow(()=>validateAssetGroups(undefined,p.assets));
});
test('project-level validation accepts legacy absence and rejects malformed source metadata or limits',()=>{
  const p=fixture();assert.doesNotThrow(()=>validateAssetGroups(p));
  const group={id:'g',name:'材'.repeat(80),frames:[{assetId:'b',ticks:1500}],source:{format:'GIF',fileNames:['角色.gif']}};
  p.assetGroups=[group];assert.doesNotThrow(()=>validateAssetGroups(p));
  for(const bad of [{...group,id:123},{...group,name:'材'.repeat(81)},{...group,frames:[{assetId:'gone',ticks:1}]},{...group,frames:[]},{...group,frames:Array.from({length:257},()=>({assetId:'b',ticks:1}))},{...group,source:{format:'',fileNames:[]}},{...group,source:{format:'PNG',fileNames:['x'.repeat(257)]}}])assert.throws(()=>validateAssetGroups({...p,assetGroups:[bad]}));
  assert.throws(()=>validateAssetGroups({...p,assetGroups:Array.from({length:257},(_,i)=>({...group,id:'g'+i}))}));
});
test('failed materialization or deletion of stale IDs never leaves partial metadata',()=>{
  const p=fixture();p.assetGroups=[{id:'taken',name:'现有',frames:[{assetId:'e',ticks:1}]}];const before=JSON.stringify(p);
  assert.throws(()=>renameAssetGroup(p,'virtual:action:walk','改名',()=> 'taken'));
  assert.throws(()=>deleteUnusedAssets(p,['e','stale']));assert.equal(JSON.stringify(p),before);
});
