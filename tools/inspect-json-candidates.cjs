'use strict';
const fs=require('fs'),a=require('internal/deps/acorn/acorn/dist/acorn'),d='E:/DESCARGAS/REA/NotDiscord/evidence/public-web/memory-factories';
const results=[];
for(const file of fs.readdirSync(d)){
 const s=fs.readFileSync(d+'/'+file,'utf8'),ast=a.parse('({'+s+'})',{ecmaVersion:'latest'}),rows=[];
 function walk(n){
  if(!n||typeof n!=='object')return;
  if(n.type==='CallExpression'&&n.callee.type==='MemberExpression'&&n.callee.object.name==='JSON'&&n.callee.property.name==='parse'&&typeof n.arguments[0]?.value==='string'){
   try{
    const v=JSON.parse(n.arguments[0].value);let objects=0,arrays=0,strings=0,chars=0;
    function count(x){if(typeof x==='string'){strings++;chars+=x.length}else if(x&&typeof x==='object'){if(Array.isArray(x))arrays++;else objects++;for(const y of Object.values(x))count(y)}}
    count(v);rows.push({jsonChars:n.arguments[0].value.length,objects,arrays,strings,stringChars:chars,lottie:!!v.layers,keys:Object.keys(v).length,start:n.start-2,end:n.end-2});
   }catch{}
  }
  for(const [k,v]of Object.entries(n))if(k!=='start'&&k!=='end'){if(Array.isArray(v))v.forEach(walk);else if(v&&typeof v==='object')walk(v)}
 }
 walk(ast);if(rows.length)results.push({file,rows});
}
fs.writeFileSync('E:/DESCARGAS/REA/NotDiscord/evidence/native/json-allocation-candidates.json',JSON.stringify(results,null,2));
console.log(JSON.stringify(results));
