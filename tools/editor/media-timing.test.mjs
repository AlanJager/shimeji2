import test from 'node:test';
import assert from 'node:assert/strict';
import {quantizeDurations,timedFrameAt} from './media-timing.mjs';

test('variable timing preserves rounded aggregate while all frames keep one tick', () => {
  assert.deepEqual(quantizeDurations([20,20,80]).ticks,[1,1,1]);
  const result=quantizeDurations([30,90,70,130]);
  assert.equal(result.ticks.reduce((a,b)=>a+b,0),8);
  assert.equal(result.originalMs,320); assert.equal(result.adjustedMs,320); assert.equal(result.changed,true);
  assert.deepEqual(quantizeDurations([40,80,120]),{ticks:[1,2,3],originalMs:240,adjustedMs:240,changed:false});
});
test('short/zero frames report unavoidable expansion, long frames remain bounded', () => {
  const short=quantizeDurations([20,20,0]); assert.deepEqual(short.ticks,[1,1,1]); assert.equal(short.adjustedMs,120); assert.equal(short.originalMs,40);
  const long=quantizeDurations([60000,20,20]); assert.equal(long.adjustedMs,60040); assert.ok(long.ticks.every(t=>t>=1&&t<=1500));
  assert.deepEqual(quantizeDurations([60000]).ticks,[1500]);
  for(const invalid of [[],[60001],[-1],[NaN],Array(257).fill(40)]) assert.throws(()=>quantizeDurations(invalid));
});
test('variable preview follows exact boundaries and finite loop behavior', () => {
  const frames=[{ticks:1},{ticks:3},{ticks:2}];
  assert.deepEqual([0,1,3,4,5,6,7].map(t=>timedFrameAt(frames,t)),[0,1,1,2,2,0,1]);
  assert.equal(timedFrameAt(frames,6,false),2); assert.equal(timedFrameAt(frames,-1,false),0);
});
test('quantization stays bounded and conserves feasible totals across varied sequences',()=>{
  let seed=17;const random=()=>{seed=(seed*1664525+1013904223)>>>0;return seed;};
  for(let caseIndex=0;caseIndex<100;caseIndex++){
    const durations=Array.from({length:1+random()%256},()=>random()%60001),original=[...durations],result=quantizeDurations(durations);
    assert.ok(result.ticks.every(tick=>Number.isInteger(tick)&&tick>=1&&tick<=1500));
    assert.equal(result.ticks.reduce((sum,tick)=>sum+tick,0),Math.max(durations.length,Math.round(durations.reduce((sum,ms)=>sum+ms,0)/40)));
    assert.deepEqual(durations,original);
  }
});
