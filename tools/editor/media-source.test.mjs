import test from 'node:test';
import assert from 'node:assert/strict';
import {validateDecodedMedia,decodeMediaFiles} from './media-source.mjs';

function frame(name='frame.png',durationMs=80,width=2,height=2) {
  const bytes=new Uint8Array(24),view=new DataView(bytes.buffer);
  view.setUint32(0,0x89504e47);view.setUint32(4,0x0d0a1a0a);view.setUint32(8,13);view.setUint32(12,0x49484452);view.setUint32(16,width);view.setUint32(20,height);
  return {name,durationMs,width,height,src:'data:image/png;base64,'+btoa(String.fromCharCode(...bytes))};
}
function file(name,path=name) {const value=new Blob(['local']);Object.defineProperties(value,{name:{value:name},webkitRelativePath:{value:path===name?'':path}});return value;}
const data=(name='image',format='png')=>({name,format,frames:[frame(name+'.png',240)],clips:[],warnings:[]});

test('decoder admission rejects outside sources, mismatched dimensions and bad tag indices',()=>{
  const input=data();assert.equal(validateDecodedMedia(input).frames[0].blob.size,24);
  for(const mutate of [x=>x.frames[0].src='https://example.org/a.png',x=>x.frames[0].width=3,x=>x.frames[0].durationMs=-1,x=>x.clips=[{name:'bad',loop:true,indices:[1]}]]) {
    const copy=structuredClone(input);mutate(copy);assert.throws(()=>validateDecodedMedia(copy));
  }
  const big=data();big.frames=[frame('large.png',40,4096,4096),frame('extra.png')];assert.throws(()=>validateDecodedMedia(big),/上限/);
});

test('folder static files use natural path order and preserve separate directory clips',async()=>{
  const original=globalThis.fetch,calls=[];
  globalThis.fetch=async(url)=>{calls.push(url);return new Response(JSON.stringify(data(decodeURIComponent(url.split('name=')[1]))));};
  try {
    const result=await decodeMediaFiles([file('10.png','walk/10.png'),file('2.png','walk/2.png'),file('1.png','idle/1.png')]);
    assert.deepEqual(result.fileNames,['idle/1.png','walk/2.png','walk/10.png']);
    assert.deepEqual(result.clips.map(c=>[c.name,c.indices]),[['idle',[0]],['walk',[1,2]]]);
    assert.equal(result.animated,false);assert.equal(result.gridEligible,false);assert.equal(calls.length,3);
  } finally {globalThis.fetch=original;}
});

test('animated clips retain source loops and offset frame indices across files',async()=>{
  const original=globalThis.fetch;
  globalThis.fetch=async url=>{const name=decodeURIComponent(url.split('name=')[1]);return new Response(JSON.stringify({format:'gif',name,frames:[frame('first',20),frame('second',90)],clips:[{name,indices:[0,1,0],loop:false}],warnings:['single cycle']}));};
  try {
    const result=await decodeMediaFiles([file('b.gif'),file('a.gif')]);
    assert.equal(result.animated,true);assert.deepEqual(result.clips.map(c=>c.indices),[[0,1,0],[2,3,2]]);assert.ok(result.clips.every(c=>!c.loop));
    assert.deepEqual(result.frames.map(f=>f.durationMs),[20,90,20,90]);assert.deepEqual(result.warnings,['single cycle']);
  } finally {globalThis.fetch=original;}
});

test('aborted and oversized requests fail before contacting decoder',async()=>{
  const original=globalThis.fetch;let calls=0;globalThis.fetch=async()=>{calls++;throw Error('must not fetch');};
  try {
    const controller=new AbortController();controller.abort();await assert.rejects(decodeMediaFiles([file('a.png')],{signal:controller.signal}),{name:'AbortError'});
    await assert.rejects(decodeMediaFiles([file('outside.svg')]),/支持/);
    const large=file('a.png');Object.defineProperty(large,'size',{value:8*1024*1024+1});await assert.rejects(decodeMediaFiles([large]),/大小/);
    assert.equal(calls,0);
  } finally {globalThis.fetch=original;}
});

test('aggregate admission stops a batch before fetching subsequent sources',async()=>{
  const original=globalThis.fetch;let calls=0;
  globalThis.fetch=async()=>{calls++;return new Response(JSON.stringify({...data('many','gif'),frames:Array.from({length:256},(_,i)=>frame(`f${i}`,40)),clips:[]}));};
  try {await assert.rejects(decodeMediaFiles([file('a.gif'),file('b.gif'),file('c.gif')]),/整批/);assert.equal(calls,2);}finally{globalThis.fetch=original;}
});

test('atlas pair posts local metadata and sheet bytes without resolving image URLs',async()=>{
  const original=globalThis.fetch;let posted;
  const json=new Blob([JSON.stringify({meta:{image:'sheet.png'}})]);Object.defineProperty(json,'name',{value:'atlas.json'});
  globalThis.fetch=async(url,options)=>{assert.equal(url,'/api/media/atlas');posted=JSON.parse(options.body);return new Response(JSON.stringify({...data('atlas','aseprite'),clips:[{name:'idle',loop:true,indices:[0]}]}));};
  try {const result=await decodeMediaFiles([json,file('sheet.png')]);assert.equal(posted.sheetName,'sheet.png');assert.equal(posted.metadata.meta.image,'sheet.png');assert.equal(result.animated,true);}finally{globalThis.fetch=original;}
});

test('folder metadata alone is skipped, accepted source names and animation timing remain intact',async()=>{
  const original=globalThis.fetch;let calls=0;
  globalThis.fetch=async()=>{calls++;return new Response(JSON.stringify({format:'gif',name:'motion',frames:[frame('a',20),frame('b',90)],clips:[{name:'motion',indices:[0,1],loop:false}],warnings:[]}));};
  try {
    const result=await decodeMediaFiles([file('.DS_Store','folder/.DS_Store'),file('._1.png','folder/._1.png'),file('hidden.png','folder/__MACOSX/hidden.png'),file('motion.gif','folder/motion.gif')]);
    assert.equal(calls,1);assert.deepEqual(result.fileNames,['folder/motion.gif']);assert.deepEqual(result.frames.map(f=>f.durationMs),[20,90]);assert.equal(result.clips[0].loop,false);assert.match(result.warnings[0],/3.*元数据/);
    await assert.rejects(decodeMediaFiles([file('notes.txt','folder/notes.txt')]),/支持/);
    await assert.rejects(decodeMediaFiles([file('.DS_Store')]),/支持/);
    const controller=new AbortController();controller.abort();await assert.rejects(decodeMediaFiles([file('.DS_Store','folder/.DS_Store'),file('motion.gif','folder/motion.gif')],{signal:controller.signal}),{name:'AbortError'});
    assert.equal(calls,1);
  }finally{globalThis.fetch=original;}
});

test('one static JPEG remains eligible for the existing grid normalizer',async()=>{
  const original=globalThis.fetch;globalThis.fetch=async()=>new Response(JSON.stringify(data('sheet','jpeg')));
  try{const result=await decodeMediaFiles([file('sheet.jpg')]);assert.equal(result.gridEligible,true);assert.equal(result.animated,false);}finally{globalThis.fetch=original;}
});

test('a ZIP can expose more than 64 valid source clips before the UI chooses actions',()=>{
  const input={...data('folders','zip'),clips:Array.from({length:65},(_,i)=>({name:`folder${i}`,loop:true,indices:[0]}))};
  assert.equal(validateDecodedMedia(input).clips.length,65);
  input.clips[64].indices=[1];assert.throws(()=>validateDecodedMedia(input),/引用/);
});
