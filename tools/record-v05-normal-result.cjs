const fs=require('node:fs');const crypto=require('node:crypto');
const root='E:/DESCARGAS/REA/NotDiscord';
function readJson(p){return JSON.parse(fs.readFileSync(root+'/'+p,'utf8').replace(/^\uFEFF/,''));}
const relative='evidence/benchmarks/Pair-v05-normal-no-internal-probe-20261017T171757662Z.json';
const run=readJson(relative),d=run.Summary.Discord,n=run.Summary.NotDiscord;
const saving=(a,b)=>100*(a-b)/a;
const resident=saving(d.PrivateWorkingSetMiB.Mean,n.PrivateWorkingSetMiB.Mean),commit=saving(d.PrivateCommitMiB.Mean,n.PrivateCommitMiB.Mean),cpu=saving(d.CpuSystemPercentWeightedMean,n.CpuSystemPercentWeightedMean);
const f=(x,places=2)=>x.toFixed(places).replace('.',',');
const normal={Scenario:run.Scenario,Source:relative,SourceSha256:crypto.createHash('sha256').update(fs.readFileSync(root+'/'+relative)).digest('hex'),SamplePairs:d.SampleCount,WarmupSecondsMinimum:120,InternalSessionProbe:false,Discord:d,NotDiscord:n,SavingsPercent:{PrivateResident:resident,PrivateCommit:commit,Cpu:cpu},FreshSideBySideConfirmation:false,TwentyFivePercentRamReductionDemonstrated:false};
const sp='evidence/optimization-v03-v04-summary.json';const summary=readJson(sp);summary.NormalLaunchComparison=normal;summary.ReportStatus='normal_launch_measured_acceptance_pending';summary.GeneratedUtc=new Date().toISOString();summary.LatestVersionStatus='0.5.0 normal launch measured externally; 25 percent resident RAM target not achieved';summary.Limitations=summary.Limitations.filter(x=>!x.includes('normal launch measured externally remains pending'));summary.MeasurementInstrumentation={PriorFourRuns:'--measure-session: periodic CDP heap/DOM diagnostics for up to 180 seconds',LatestRun:'Normal launch without --measure-session; external Windows process-tree counters only',NoCausalConclusionOnProbeOverhead:true};fs.writeFileSync(root+'/'+sp,JSON.stringify(summary,null,2)+'\n');
const evidenceLink='../'+relative;
const normalBlock=`## Medición más reciente: arranque normal\n\nSin sondeo interno, tras al menos 120 segundos de estabilización y con 61 pares durante un minuto: **NotDiscord ${f(n.PrivateWorkingSetMiB.Mean)} MiB frente a Discord ${f(d.PrivateWorkingSetMiB.Mean)} MiB de RAM privada residente (${f(-resident)} % más)**. Memoria comprometida: ${f(n.PrivateCommitMiB.Mean)} frente a ${f(d.PrivateCommitMiB.Mean)} MiB (${f(commit)} % menos). CPU: ${f(n.CpuSystemPercentWeightedMean,5)} % frente a ${f(d.CpuSystemPercentWeightedMean,5)} % del equipo.\n\nSe incluyeron ocho procesos de NotDiscord y seis de Discord, sin errores de lectura. Ambas ventanas estaban visibles y sin minimizar; Discord tuvo el foco. Falta confirmar colocación lado a lado. **El objetivo de 25 % menos RAM no se alcanza.** Retirar la sonda no demostró el ahorro requerido; estas observaciones no aíslan su coste. [Muestra externa](${evidenceLink}).\n\n`;
for(const path of ['README.md','PLAN.md','docs/VALIDACION.md','docs/OPTIMIZACION-V03-V05.md']){
 let s=fs.readFileSync(root+'/'+path,'utf8').replace(/^\uFEFF/,'');
 s=s.replace(/\*\*Límite de instrumentación:\*\*[^\n]*\n\n/,'');
 if(path==='README.md'){
  s=s.replace('La última muestra instrumentada disponible,','La última muestra de arranque normal, sin sonda interna,');
  for(const [a,b] of [['314,72',f(d.PrivateWorkingSetMiB.Mean)],['358,49',f(n.PrivateWorkingSetMiB.Mean)],['611,10',f(d.PrivateCommitMiB.Mean)],['496,55',f(n.PrivateCommitMiB.Mean)],['0,03490',f(d.CpuSystemPercentWeightedMean,5)],['0,03960',f(n.CpuSystemPercentWeightedMean,5)],['18,75 %',f(commit)+' %'],['13,91 %',f(-resident)+' %']])s=s.replaceAll(a,b);
  s=s.replace('Memoria comprometida y RAM residente son métricas distintas.','Memoria comprometida y RAM residente son métricas distintas. Se esperaron al menos 120 segundos antes de medir con contadores externos de Windows. [Muestra normal]('+relative+').');
 }else{
  const firstBreak=s.indexOf('\n');s=s.slice(0,firstBreak+1)+'\n'+normalBlock.replace(evidenceLink,path==='PLAN.md'?relative:evidenceLink)+s.slice(firstBreak+1).replace(/^\n/,'');
  s=s.replace('Último resultado instrumentado y límites','Resultado instrumentado anterior y límites').replace('Último resultado instrumentado','Resultado instrumentado anterior').replace('Última medición autenticada instrumentada','Medición autenticada instrumentada anterior').replace('La serie más reciente muestra','La serie instrumentada anterior mostró');
  if(path.endsWith('OPTIMIZACION-V03-V05.md')){
   s=s.replace('resultado instrumentado de la candidata','resultados de la candidata');
   s=s.replace(/(\| v05 \|[^\n]*\n)/,`$1| v05 normal, sin sonda | ${f(d.PrivateWorkingSetMiB.Mean)} / ${f(n.PrivateWorkingSetMiB.Mean)} | −${f(-resident)} % | ${f(d.PrivateCommitMiB.Mean)} / ${f(n.PrivateCommitMiB.Mean)} | +${f(commit)} % | ${f(d.CpuSystemPercentWeightedMean,4)} / ${f(n.CpuSystemPercentWeightedMean,4)} | −${f(-cpu)} % |\n`);
  }
 }
 fs.writeFileSync(root+'/'+path,s);
}
const mp='evidence/release-manifest.json',manifest=readJson(mp);manifest.LatestNormalLaunchMeasurement={Source:relative,NotDiscordResidentMiB:n.PrivateWorkingSetMiB.Mean,DiscordResidentMiB:d.PrivateWorkingSetMiB.Mean,ResidentSavingsPercent:resident,CommittedSavingsPercent:commit,InternalProbe:false,TargetMet:false};manifest.Limitations=manifest.Limitations.filter(x=>!x.startsWith('Latest paired sample:'));manifest.Limitations.push('Latest normal launch still uses more private resident RAM than Discord; see LatestNormalLaunchMeasurement');fs.writeFileSync(root+'/'+mp,JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify({residentSavingsPercent:resident,commitSavingsPercent:commit,cpuSavingsPercent:cpu,samplePairs:d.SampleCount,filesUpdated:6}));
