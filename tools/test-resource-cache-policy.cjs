'use strict';
const assert=require('assert/strict'),install=require('../native-candidate/host/renderer-cache-policy.cjs');
async function run(){
 let cases=0;
 function setup({size=24*1024*1024,live=1024,guard={ready:true,checked:true,inVoice:false},origin='https://discord.com',isTop=true,enabled=true}={}){
  let cleared=0,scheduled=0,delay=0,calls=0;const listeners=new Map(),sent=[];
  const api=install({origin,isTop,enabled,webFrame:{getResourceUsage:()=>({scripts:{count:3,size:cleared?live:size,liveSize:live}}),executeJavaScript:async()=>{calls++;return guard},clearCache:()=>{cleared++}},ipcRenderer:{on:(k,f)=>listeners.set(k,f),send:(...x)=>sent.push(x)},setTimer:(fn,ms)=>{scheduled++;delay=ms;return{unref(){}}}});
  return{api,listeners,sent,get cleared(){return cleared},get scheduled(){return scheduled},get delay(){return delay},get calls(){return calls}};
 }
 const a=setup();assert.equal(a.delay,120000);let r=await a.api.sweep();assert.equal(r.cleared,true);assert.equal(a.cleared,1);assert(r.after.unusedBytes<r.before.unusedBytes);await a.api.sweep();assert.equal(a.cleared,1);cases++;
 for(const guard of [{ready:false,checked:true,inVoice:false},{ready:true,checked:false,inVoice:false},{ready:true,checked:true,inVoice:true}]){const a=setup({guard});const r=await a.api.sweep();assert.equal(r.cleared,false);assert.equal(a.cleared,0);cases++}
 const small=setup({size:1024});await small.api.sweep();assert.equal(small.calls,0);assert.equal(small.cleared,0);cases++;
 for(const opts of [{origin:'https://example.com'},{isTop:false}]){const a=setup(opts);assert.equal(a.api,null);assert.equal(a.listeners.size,0);assert.equal(a.scheduled,0);cases++}
 const disabled=setup({enabled:false});assert.equal(disabled.scheduled,0);disabled.listeners.get('notdiscord:resource-usage-request')({},undefined);assert.equal(disabled.cleared,0);assert.equal(disabled.sent[0][1].scripts.count,3);cases++;
 const fs=require('fs'),path=require('path');const evidence={utc:new Date().toISOString(),passed:true,cases,checks:['one sweep maximum','idle startup delay','active voice blocks sweep','unready page blocks sweep','missing voice API blocks sweep','size threshold','origin/top frame restriction','read-only report does not sweep']};
 fs.writeFileSync(path.resolve(__dirname,'../evidence/native/resource-cache-policy-tests.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify(evidence));
}run().catch(e=>{console.error(e);process.exitCode=1});
