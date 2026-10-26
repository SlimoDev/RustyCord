'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),{execFileSync}=require('child_process');
const root=path.resolve(__dirname,'..'),names=process.argv.slice(2);assert.equal(names.length,2);
const rows=JSON.parse(execFileSync(process.execPath,[path.join(__dirname,'summarize-memory-pair.cjs'),...names],{encoding:'utf8'}));
assert(rows.every(r=>r.comparisonEligible),'Both series must be eligible');
const sum=rows.reduce((n,r)=>n+r.discordMiB.Count,0);
const d=rows.reduce((n,r)=>n+r.discordMiB.Mean*r.discordMiB.Count,0)/sum,n=rows.reduce((n,r)=>n+r.notDiscordMiB.Mean*r.notDiscordMiB.Count,0)/sum;
const audit=JSON.parse(fs.readFileSync(path.join(root,'evidence/native/memory-current-build-audit.json'),'utf8'));
assert(audit.passed);
const synthetic=JSON.parse(fs.readFileSync(path.join(root,'evidence/native/media-selftest.json'),'utf8'));
const report={schema:1,utc:new Date().toISOString(),displayTimeZone:'America/Lima',
status:rows.every(r=>r.measuredMeanMeets25Percent)?'Idle RAM target measured in both series; real functional recheck pending':'25 percent target not met in both series',
metric:'Private resident RAM summed over the full process tree (PrivateWorkingSetMiB)',
series:rows,combined:{pairedSamples:sum,discordMiB:d,notDiscordMiB:n,reductionPercent:100*(1-n/d),
allPairsAtLeast25:rows.every(r=>r.sampleReduction.pairsAtLeast25===r.sampleReduction.totalPairs)},
measuredIdleTargetMet:rows.every(r=>r.measuredMeanMeets25Percent),guaranteedAcrossAllWorkloads:false,
sameSessionRepeats:true,independentRestartReplications:1,
changes:['300 unreachable preload SDK factories removed','656 bootstrap SDK factories and two telemetry capture calls removed','V8 free-page reuse pool disabled','Unused bytecode age threshold lowered from 60 to 8 seconds at ordinary collections'],
notUsed:['Forced GC','Working-set trimming','Artificial memory pressure','Disabling required Vencord plugins','Hard heap size limit'],
activeArtifacts:audit.actual,
functional:{nativeModulesPresent:audit.modules.length,requestedPluginsEnabled:audit.requestedPlugins,syntheticAudioVideoPassed:synthetic.result.success,syntheticBuildFlags:synthetic.jsFlags,realChatVoiceScreenPluginsRecheckPending:true,priorUserConfirmation:'evidence/native/user-functional-validation.json'},
rejectedExperiment:'evidence/native/resource-cache-experiment-rejected.json',
rejectedWindowStateSeries:'evidence/benchmarks/Pair-memory-code-age8-no-pressure-20261018T020649736Z.json',
notes:['User reported matching channel and no call; NotDiscord session/channel booleans confirmed.','Both windows visible and similarly sized, Discord foreground in recorded comparison.','Two repeats from the same pair of closely timed starts; not two independent launches.','Voice/screen sharing and later navigation may use more RAM and require separate benchmarks.']};
const confirmationRel='evidence/native/user-functional-validation-20261018.json';
const confirmationFile=path.join(root,confirmationRel);
if(fs.existsSync(confirmationFile)){const c=JSON.parse(fs.readFileSync(confirmationFile,'utf8'));if(Object.entries(report.activeArtifacts).every(([k,v])=>c.buildArtifacts?.[k]===v)&&c.realChatConfirmed&&c.realVoiceConfirmed&&c.realScreenShareConfirmed&&c.usualPluginsConfirmed){report.functional.realChatVoiceScreenPluginsRecheckPending=false;report.functional.userConfirmation=confirmationRel;report.functional.confirmedByUser={chat:true,voice:true,screenSharing:true,usualPlugins:true,all26IndividuallyAudited:false};report.status=report.measuredIdleTargetMet?'Idle RAM target measured in both series; user confirms chat, voice, screen sharing and usual plugins work':'25 percent target not met; user functional confirmation available';}}
const output=path.join(root,'evidence/native/memory-final-comparison-20261018.json');
fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({output,combined:report.combined,measuredIdleTargetMet:report.measuredIdleTargetMet,realFunctionalRecheckPending:report.functional.realChatVoiceScreenPluginsRecheckPending}));
