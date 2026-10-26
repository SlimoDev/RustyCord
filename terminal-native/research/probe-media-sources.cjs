'use strict';
// Enumerates source metadata with 1x1 previews. Never emits names, handles, titles or images.
const fs=require('node:fs'),path=require('node:path');
const {nativeEngine}=require('../media-helper/native.cjs');
const dir=path.join(__dirname,'fake-media-state');fs.mkdirSync(dir,{recursive:true});
const m=nativeEngine(),report={utc:new Date().toISOString(),fakeAudioVideoDevices:true,connectionsCreated:0,previewSize:[1,1]};
let allocations=0;
const timer=setTimeout(()=>{report.error='enumeration callback timeout';save(1);},12000);
function save(code){clearTimeout(timer);report.allocations=allocations;fs.writeFileSync(path.join(__dirname,'media-sources-schema.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));process.exit(code);}
function describe(values){
  if(!Array.isArray(values))return {valueType:typeof values};
  const describeOne=v=>({keys:Object.keys(v).sort(),idType:typeof v.id,idFormat:typeof v.id==='string'?v.id.replace(/[0-9]+/g,'<number>').replace(/[^:<>a-z_-]/gi,'?'):null,fieldTypes:Object.fromEntries(Object.entries(v).map(([k,a])=>[k,ArrayBuffer.isView(a)?'typed-array':Array.isArray(a)?'array':typeof a]))});
  return {count:values.length,schemas:[...new Set(values.map(v=>JSON.stringify(describeOne(v))))].map(s=>JSON.parse(s))};
}
try{
  m.setImageDataAllocator((width,height)=>{allocations++;return {width,height,data:new Uint8ClampedArray(width*height*4)};});
  m.initialize({audioSubsystem:'experimental',logLevel:-1,dataDirectory:dir,logDirectory:dir,maxLogBytes:65536,offloadAdmControls:true,asyncVideoInputDeviceInit:true,useFakeAudioCapture:true,useFakeVideoCapture:true});
  let count=0;
  function done(){if(++count===2)save(0);}
  m.getScreenPreviews(1,1,values=>{report.screens=describe(values);done();});
  m.getWindowPreviews(1,1,values=>{report.windows=describe(values);done();});
}catch(e){report.errorType=e?.constructor?.name||'Error';save(1);}
