'use strict';
const fs=require('fs'),path=require('path'),crypto=require('crypto'),assert=require('assert/strict');
const {unpack}=require('./prepare-native-bootstrap.cjs'),{parse,entryReach}=require('./prepare-native-sentry-imports.cjs');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate'),active=unpack(fs.readFileSync(path.join(native,'runtime/resources/standalone_modules/discord_desktop_core/core.asar')));
const source=active.files.get('mainScreenPreload.js').bytes.toString('utf8');
const g=parse(source),reachable=entryReach(g),dead=[...g.modules.values()].filter(x=>!reachable.has(x.id));
let forms={},odd=[];
function walk(n,p){if(!n||typeof n!=='object')return;
 if(n.type==='Identifier'&&n.name==='__webpack_require__'){
  let form=p.type;
  if(p.type==='CallExpression')form+=p.callee===n?':direct-call':':argument';
  if(p.type==='MemberExpression')form+=':'+p.property?.name;
  forms[form]=(forms[form]||0)+1;
  if(!['FunctionDeclaration','FunctionExpression','ArrowFunctionExpression','MemberExpression','CallExpression'].includes(p.type))odd.push({kind:p.type,source:source.slice(p.start,p.end).slice(0,250)});
 }
 for(const [key,v]of Object.entries(n)){if(key==='start'||key==='end')continue;if(Array.isArray(v))v.forEach(x=>walk(x,n));else if(v&&typeof v==='object')walk(v,n)}
}
walk(g.ast,null);
const report={sha256:crypto.createHash('sha256').update(source).digest('hex'),moduleCount:g.modules.size,reachable:reachable.size,unreachable:dead.length,unreachableBytes:dead.reduce((s,x)=>s+x.bytes,0),dynamicCalls:g.dynamic,loaderIdentifierForms:forms,odd};
fs.writeFileSync(path.join(root,'evidence/native/preload-dead-code-audit.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
