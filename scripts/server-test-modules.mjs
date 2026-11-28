/** Compile actual server adapters into a private disposable test directory. */
import {mkdtemp,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {createRequire,isBuiltin} from 'node:module';
import {resolve,dirname,join} from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import ts from '../web/node_modules/typescript/lib/typescript.js';
const root=fileURLToPath(new URL('../',import.meta.url));
const webRequire=createRequire(new URL('../web/package.json',import.meta.url));
export async function serverModules(){
 const directory=await mkdtemp(join(tmpdir(),'supa-server-modules-'));const cache=new Map();
 async function compile(path){
  if(cache.has(path))return cache.get(path);
  const target=join(directory,cache.size+'.mjs');const url=pathToFileURL(target).href;cache.set(path,url);
  let source=await readFile(path,'utf8');
  const imports=[...source.matchAll(/\bfrom\s*["']([^"']+)["']/g)];
  for(const match of imports){const specifier=match[1];let dest;
   if(isBuiltin(specifier))continue;
   if(specifier==='zod')dest=pathToFileURL(resolve(root,'web/node_modules/zod/index.js')).href;
   else if(specifier==='next/server')dest=pathToFileURL(resolve(root,'web/node_modules/next/server.js')).href;
   else if(!specifier.startsWith('@/')&&!specifier.startsWith('./')&&!specifier.startsWith('../'))dest=pathToFileURL(webRequire.resolve(specifier)).href;
   else {const p=specifier.startsWith('@/')?resolve(root,'web/src',specifier.slice(2)):resolve(dirname(path),specifier);dest=p.endsWith('.mjs')?pathToFileURL(p).href:await compile(p.endsWith('.ts')?p:p+'.ts');}
   source=source.replace(match[0],match[0].replace(specifier,dest));
  }
  const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  await writeFile(target,output,{mode:0o600});return url;
 }
 const cleanup=()=>rm(directory,{recursive:true,force:true});
 return {directory,load:async(relative)=>{try{return await import(await compile(resolve(root,relative)));}catch(error){await cleanup();throw error;}},cleanup};
}
