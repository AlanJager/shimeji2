import test from 'node:test';
import assert from 'node:assert/strict';
import {prepareMediaImport} from './media-integration.mjs';
const png = 'data:image/png;base64,cG5n';
function fixture() {
  const project = {assets:[{id:'a',name:'original',src:png}],parts:[],actions:[{id:'idle',kind:'frames',frames:[{id:'f',assetId:'a',ticks:7,x:0,y:0}]}],rules:[{id:'r',actionId:'idle'}]};
  const payload = {projectToken:project,destination:'new',groupName:'walk',assets:[{name:'a.png',src:png},{name:'b.png',src:png+'Yg=='}],
    clips:[{name:'forward',loop:true,frames:[{assetIndex:0,ticks:1},{assetIndex:1,ticks:3}]},{name:'reverse',loop:false,frames:[{assetIndex:1,ticks:3},{assetIndex:0,ticks:1}]}],groupFrames:[{assetIndex:0,ticks:1},{assetIndex:1,ticks:3}],source:{format:'aseprite',fileNames:['a.json','a.png']}};
  let id = 0; return {project,payload,prepare:() => prepareMediaImport(project,payload,'idle',prefix=>prefix+'-'+(++id))};
}
test('all tags share deduplicated pixels while keeping independent order and times', () => {
  const f=fixture(),original=structuredClone(f.project),prepared=f.prepare();
  assert.deepEqual(f.project,original);prepared.apply(f.project);
  assert.equal(f.project.assets.length,2);assert.equal(f.project.actions.length,3);
  assert.deepEqual(f.project.actions[1].frames.map(x=>x.ticks),[1,3]);
  assert.equal(f.project.actions[1].frames[0].assetId,'a');
  assert.equal(f.project.actions[1].frames[1].assetId,f.project.actions[2].frames[0].assetId);
  assert.equal(f.project.assetGroups[0].frames.length,2);
  assert.deepEqual(f.project.rules,original.rules);
});
test('invalid later tag and stale project reject before any partial mutation', () => {
  const f=fixture(),original=JSON.stringify(f.project);
  f.payload.clips[1].frames[0].assetIndex=9;assert.throws(f.prepare,/引用/);assert.equal(JSON.stringify(f.project),original);
  f.payload.projectToken={};assert.throws(f.prepare,/工程已变化/);
});
test('replacement preserves asset ID, name, every frame duration and rules', () => {
  const f=fixture(),frames=structuredClone(f.project.actions),rules=structuredClone(f.project.rules);
  Object.assign(f.payload,{destination:'replace',replaceAssetId:'a',assets:[{name:'new.jpg',src:png+'Yg=='}]});f.prepare().apply(f.project);
  assert.deepEqual(f.project.actions,frames);assert.deepEqual(f.project.rules,rules);
  assert.deepEqual(f.project.assets,[{id:'a',name:'original',src:png+'Yg=='}]);assert.equal(f.project.assetGroups,undefined);
});
test('capacity uses unique PNGs and rejects overflow or animated parts stacking', () => {
  const f=fixture();f.project.assets.push(...Array.from({length:255},(_,i)=>({id:'x'+i,src:'x'+i})));assert.throws(f.prepare,/256/);
  f.project.assets.splice(1);f.project.actions[0].kind='parts';f.payload.clips.splice(1);f.payload.destination='append';f.payload.animated=true;
  assert.throws(f.prepare,/静态部件/);
});
test('a selected static-folder clip only adds its referenced part images', () => {
  const f=fixture();f.project.actions[0].kind='parts';f.payload.destination='append';f.payload.clips=[{name:'selected',loop:true,frames:[{assetIndex:1,ticks:6}]}];
  const result=f.prepare();assert.equal(result.partAssets.length,1);assert.equal(result.partAssets[0].name,'b.png');
});
