'use strict';
const fs=require('fs'),path=require('path'),root=path.resolve(__dirname,'..');
const file=path.join(root,'evidence/benchmarks/Pair-memory-resource-sweep-timeline-20261018T012815523Z.json');
const d=JSON.parse(fs.readFileSync(file,'utf8'));
const sweep=JSON.parse(fs.readFileSync(path.join(root,'native-candidate/logs/resource-cache-sweep-22060.json'),'utf8')),at=Date.parse(sweep.utc);
function stats(rows,key){const vals=rows.map(r=>r[key]).filter(v=>typeof v==='number').sort((a,b)=>a-b);return{n:vals.length,mean:vals.reduce((a,b)=>a+b,0)/vals.length,median:vals[Math.floor(vals.length/2)],min:vals[0],max:vals.at(-1)}}
const out={utc:new Date().toISOString(),benchmark:path.relative(root,file),sweepUtc:sweep.utc,comparisonInvalid:'Discord was minimized throughout this sample; not proof of target saving.',before:{},after:{},perTenSeconds:[]};
for(const [label,begin,end]of [['before',at-31000,at-1000],['after',at+5000,at+35000]]){
 for(const client of ['Discord','NotDiscord']){
  const rows=d.Samples.filter(r=>r.Client===client&&Date.parse(r.TimestampUtc)>=begin&&Date.parse(r.TimestampUtc)<end);
  out[label][client]={privateResidentMiB:stats(rows,'PrivateWorkingSetMiB'),privateCommitMiB:stats(rows,'PrivateCommitMiB'),cpuSystemPercent:stats(rows,'CpuSystemPercent')};
 }
}
for(let sec=0;sec<=120;sec+=10){const r=d.Samples.find(r=>r.Client==='NotDiscord'&&r.ElapsedSeconds>=sec);if(r)out.perTenSeconds.push({utc:r.TimestampUtc,MiB:r.PrivateWorkingSetMiB,cpu:r.CpuSystemPercent})}
out.observedNativeResidentDropMiB=out.before.NotDiscord.privateResidentMiB.mean-out.after.NotDiscord.privateResidentMiB.mean;
fs.writeFileSync(path.join(root,'evidence/native/resource-cache-timeline-analysis.json'),JSON.stringify(out,null,2));
console.log(JSON.stringify(out));
