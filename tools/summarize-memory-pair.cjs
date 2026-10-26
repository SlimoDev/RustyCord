'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict');
const root=path.resolve(__dirname,'..'),bench=path.join(root,'evidence','benchmarks');
const names=process.argv.slice(2);assert(names.length>0);
const reports=names.map(name=>{
 assert.equal(path.basename(name),name);assert(name.endsWith('.json'));
 const m=JSON.parse(fs.readFileSync(path.join(bench,name),'utf8')),d=m.Summary.Discord,n=m.Summary.NotDiscord;
 const starts=m.Roots.map(x=>new Date(x.StartUtc).getTime()),gapSeconds=(Math.max(...starts)-Math.min(...starts))/1000;
 const hidden=m.Samples.filter(s=>!s.TreeAnyVisible||s.TreeAnyMinimized).length;
 const unavailable=m.Samples.filter(s=>s.UnreadableProcessCount||s.UnavailablePrivateWorkingSetCount||s.InvalidPrivateWorkingSetCount).length;
 const rejectedByPressure=/^memory-(resource-sweep-timeline|cache-matched-restarts|v8-page-pool-off|pruned-preload-pool-off|bootstrap-sdk-pruned)$/.test(m.Scenario);
 const savings=100*(1-n.PrivateWorkingSetMiB.Mean/d.PrivateWorkingSetMiB.Mean);
 const pairs=new Map();for(const sample of m.Samples){if(!pairs.has(sample.Sample))pairs.set(sample.Sample,{});pairs.get(sample.Sample)[sample.Client]=sample}
 const pct=[...pairs.values()].filter(p=>p.Discord?.PrivateWorkingSetMiB>0&&p.NotDiscord?.PrivateWorkingSetMiB>=0).map(p=>100*(1-p.NotDiscord.PrivateWorkingSetMiB/p.Discord.PrivateWorkingSetMiB)).sort((a,b)=>a-b);
 const eligible=m.ExitReason==='duration_completed'&&m.Roots.length===2&&!hidden&&!unavailable&&!rejectedByPressure&&gapSeconds<=30&&m.ActualDurationSeconds>=59;
 return {source:'evidence/benchmarks/'+name,scenario:m.Scenario,startedUtc:m.StartedUtc,endedUtc:m.EndedUtc,exitReason:m.ExitReason,actualDurationSeconds:m.ActualDurationSeconds,
 startGapSeconds:gapSeconds,hiddenOrMinimizedRows:hidden,unavailableRows:unavailable,rejectedByPressure,comparisonEligible:eligible,
 discordMiB:d.PrivateWorkingSetMiB,notDiscordMiB:n.PrivateWorkingSetMiB,reductionPercent:savings,
 sampleReduction:{min:pct[0],median:pct[Math.floor(pct.length/2)],max:pct.at(-1),pairsAtLeast25:pct.filter(x=>x>=25).length,totalPairs:pct.length},
 cpuSystemPercent:{Discord:d.CpuSystemPercentWeightedMean,NotDiscord:n.CpuSystemPercentWeightedMean},
 measuredMeanMeets25Percent:eligible&&savings>=25,
 limitations:['Same channel and no call are user-reported, with NotDiscord channel corroborated by a boolean probe.','No independent occlusion detection or activity control.','An idle measurement does not establish savings during voice, sharing, navigation, or all future sessions.']};
});
console.log(JSON.stringify(reports,null,2));
