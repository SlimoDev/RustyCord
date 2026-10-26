'use strict';
const fs=require('node:fs'),path=require('node:path'),net=require('node:net'),crypto=require('node:crypto'),{spawn}=require('node:child_process');
const executable=path.resolve(__dirname,'../media-host/runtime/NotDiscord.Media.exe');
async function check(endWithEof){return new Promise(resolve=>{
  const pipe='\\\\.\\pipe\\NotDiscord.Media.'+crypto.randomUUID();
  const server=net.createServer();let child,socket,buffer='',sent=false;
  const report={termination:endWithEof?'pipe-eof':'shutdown',ready:false,capabilities:false,stats:false,nativeInitialized:false,backend:null,exitCode:null,invalidProtocolLines:0,stdoutBytes:0,stderrBytes:0,timedOut:false};
  const timer=setTimeout(()=>{report.timedOut=true;socket?.destroy();child?.kill();server.close();},5000);
  server.on('connection',peer=>{
    if(socket){peer.destroy();return;}socket=peer;server.close();peer.setEncoding('utf8');peer.on('error',()=>{});
    peer.on('data',chunk=>{buffer+=chunk;let at;while((at=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,at).trim();buffer=buffer.slice(at+1);if(!line)continue;try{const frame=JSON.parse(line);if(frame.event==='ready'&&!sent){report.ready=true;sent=true;peer.write(JSON.stringify({id:'caps',command:'capabilities'})+'\n');peer.write(JSON.stringify({id:'stats',command:'stats'})+'\n');}else if(frame.event==='mediaStage')report.nativeInitialized=true;else if(frame.id==='caps'){report.capabilities=frame.ok&&frame.result?.voice===true;report.backend=frame.result?.backend??null;}else if(frame.id==='stats'){report.stats=frame.ok&&frame.result?.sanitized===true&&frame.result.voice===null&&frame.result.share===null;if(endWithEof)peer.end();else peer.write(JSON.stringify({id:'bye',command:'shutdown'})+'\n');}}catch{report.invalidProtocolLines++;}}});
  });
  server.listen(pipe,()=>{
    child=spawn(executable,[],{windowsHide:true,env:{...process.env,NOTDISCORD_MEDIA_PIPE:pipe},stdio:['ignore','pipe','pipe']});
    child.stdout.on('data',chunk=>report.stdoutBytes+=chunk.length);child.stderr.on('data',chunk=>report.stderrBytes+=chunk.length);
    child.on('exit',code=>{clearTimeout(timer);report.exitCode=code;socket?.destroy();server.close();resolve(report);});
  });
});}
(async()=>{const checks=[await check(false),await check(true)];const report={utc:new Date().toISOString(),realNetworkUsed:false,devicesInitialized:false,success:checks.every(x=>x.ready&&x.capabilities&&x.stats&&x.exitCode===0&&!x.nativeInitialized&&!x.timedOut&&x.invalidProtocolLines===0),checks};fs.writeFileSync(path.join(__dirname,'media-host-ipc.json'),JSON.stringify(report,null,2));process.stdout.write(JSON.stringify(report,null,2)+'\n');process.exitCode=report.success?0:1;})();
