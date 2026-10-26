'use strict';
const fs=require('node:fs'),path=require('node:path'),{spawnSync}=require('node:child_process');
const executable=path.resolve(__dirname,'../media-host/runtime/NotDiscord.Media.exe');
const result=spawnSync(executable,['--media-probe-mls'],{encoding:'utf8',windowsHide:true,timeout:7000,maxBuffer:1024*1024});
const stages=[];let unknownStdoutLines=0;
for(const line of(result.stdout||'').split(/\r?\n/)){if(!line)continue;try{const item=JSON.parse(line);if(typeof item.stage==='string'){stages.push(item);continue;}}catch{}unknownStdoutLines++;}
const report={utc:new Date().toISOString(),fakeCapture:true,loopbackOnly:true,originalUiLoaded:false,browserWindowsCreated:0,exitCode:result.status,signal:result.signal,stages,unknownStdoutLines,stderrBytes:Buffer.byteLength(result.stderr||''),timedOut:result.error?.code==='ETIMEDOUT'};
fs.writeFileSync(path.join(__dirname,'media-host-mls.json'),JSON.stringify(report,null,2));
fs.writeFileSync(path.join(__dirname,'media-host-synthetic-stderr.txt'),result.stderr||'');
process.stdout.write(JSON.stringify(report,null,2)+'\n');process.exitCode=result.status===0?0:1;
