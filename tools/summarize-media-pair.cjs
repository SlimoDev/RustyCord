'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict');
const root=path.resolve(__dirname,'..'),[filename,conditionsName]=process.argv.slice(2);
for(const name of [filename,conditionsName]){assert(name&&path.basename(name)===name&&name.endsWith('.json'));}
const m=JSON.parse(fs.readFileSync(path.join(root,'evidence/benchmarks',filename),'utf8'));
const c=JSON.parse(fs.readFileSync(path.join(root,'evidence/native',conditionsName),'utf8'));
assert(['voice','screen-share'].includes(c.phase));assert.equal(c.scenario,m.Scenario);
const roots=m.Roots.map(r=>new Date(r.StartUtc).getTime());
const startGapSeconds=(Math.max(...roots)-Math.min(...roots))/1000;
const badRows=m.Samples.filter(s=>!s.TreeAnyVisible||s.TreeAnyMinimized||s.UnreadableProcessCount||s.UnavailablePrivateWorkingSetCount||s.InvalidPrivateWorkingSetCount).length;
const full=m.ExitReason==='duration_completed'&&m.ActualDurationSeconds>=119&&m.Roots.length===2;
const mediaComparable=c.bothClientsActiveConfirmed===true&&c.sameAudioSettingsConfirmed===true&&c.sameCallConfirmed===true&&c.cameraOffConfirmed===true&&(c.phase==='voice'?(c.screenShareOffConfirmed===true&&c.receivingAudioBothConfirmed===true):(c.bothTransmittingConfirmed===true&&c.sameSourceConfirmed===true&&c.sameResolutionFpsConfirmed===true&&c.samePreviewAndReceiveStateConfirmed===true));
const finiteRam=Number.isFinite(m.Summary.Discord.PrivateWorkingSetMiB.Mean)&&m.Summary.Discord.PrivateWorkingSetMiB.Mean>0&&Number.isFinite(m.Summary.NotDiscord.PrivateWorkingSetMiB.Mean)&&m.Summary.NotDiscord.PrivateWorkingSetMiB.Mean>=0;
const numericEligible=full&&badRows===0&&startGapSeconds<=30&&finiteRam;
const perClient={};
for(const client of ['Discord','NotDiscord']){
 const sum=m.Summary[client],samples=m.Samples.filter(s=>s.Client===client),cpu=samples.map(s=>s.CpuSystemPercent).filter(Number.isFinite).sort((a,b)=>a-b);
 const newProcesses=samples.reduce((n,s)=>n+(s.NewProcessesWithoutCpuBaseline||0),0);
 const seen=new Set(m.ProcessesSeen.filter(p=>p.Client===client).map(p=>p.ProcessId+':'+p.StartUtc));
 const membershipStable=seen.size===sum.ProcessCount.Min&&sum.ProcessCount.Min===sum.ProcessCount.Max;
 perClient[client]={ramMiB:sum.PrivateWorkingSetMiB,privateCommitMiB:sum.PrivateCommitMiB,cpu:{weightedMeanPercent:sum.CpuSystemPercentWeightedMean,p95Percent:cpu.length?cpu[Math.max(0,Math.ceil(cpu.length*.95)-1)]:null,peakPercent:cpu.at(-1)??null,newProcessIntervalsWithoutBaseline:newProcesses,membershipStable,validForComparison:membershipStable&&newProcesses===0&&sum.UnreadableProcessObservations===0},processCount:sum.ProcessCount};
}
const d=perClient.Discord,n=perClient.NotDiscord,ramReduction=100*(1-n.ramMiB.Mean/d.ramMiB.Mean);
const cpuReduction=d.cpu.weightedMeanPercent>0?100*(1-n.cpu.weightedMeanPercent/d.cpu.weightedMeanPercent):null;
const comparable=numericEligible&&mediaComparable,cpuComparable=comparable&&d.cpu.validForComparison&&n.cpu.validForComparison&&Number.isFinite(d.cpu.weightedMeanPercent)&&d.cpu.weightedMeanPercent>=0&&Number.isFinite(n.cpu.weightedMeanPercent)&&n.cpu.weightedMeanPercent>=0;
const report={schema:1,utc:new Date().toISOString(),phase:c.phase,source:'evidence/benchmarks/'+filename,conditions:'evidence/native/'+conditionsName,scenario:m.Scenario,notes:m.Notes,startedUtc:m.StartedUtc,endedUtc:m.EndedUtc,durationSeconds:m.ActualDurationSeconds,startGapSeconds,invalidVisibilityOrMemoryRows:badRows,numericEligible,mediaComparable,comparisonEligible:comparable,clients:perClient,
ram:{reductionPercent:ramReduction,meets25Percent:comparable&&ramReduction>=25},
cpu:{unit:'percent of total logical CPU capacity',comparable:cpuComparable,reductionPercent:cpuReduction,differencePercentagePoints:n.cpu.weightedMeanPercent-d.cpu.weightedMeanPercent,meets25Percent:cpuComparable&&cpuReduction!==null&&cpuReduction>=25,baselineNearZero:d.cpu.weightedMeanPercent<0.1},
limitations:['Call and screen-share activity/settings are user-confirmed, not automatically inspected.','Both clients run concurrently; results may differ when each runs alone.','No GPU utilization or external shared-system allocations measured.','This measures mean savings for the recorded workload, not a permanent minimum.']};
const output=path.join(root,'evidence/native',c.phase+'-comparison-'+m.StartedUtc.replace(/[^0-9]/g,'').slice(0,14)+'.json');
fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({output,...report},null,2));
