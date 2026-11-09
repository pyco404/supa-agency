/** Offline, source-derived video candidates. Never enables production playback. */
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync,mkdirSync,writeFileSync,existsSync} from 'node:fs';
import {resolve} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const refresh=process.argv.includes('--playback-refresh');
const articulated=process.argv.includes('--articulated');
const only=process.argv.find(arg=>arg.startsWith('--only='))?.slice(7);
if(process.argv.slice(2).some(arg=>!['--playback-refresh','--articulated',`--only=${only}`].includes(arg))||(refresh&&articulated)||(only&&!['factory','economy','social','trading'].includes(only)))throw Error('unsupported_motion_argument');
const output=resolve(root,articulated?'artifacts/navigation-motion-articulated':refresh?'artifacts/navigation-motion-playback-refresh':'reference-web/motion-candidates');
const work=resolve(output,'work');
mkdirSync(output,{recursive:true});mkdirSync(work,{recursive:true});
const sections=['factory','economy','social','trading'];
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
function run(args,timeout=300000){const r=spawnSync('ffmpeg',['-hide_banner','-loglevel','error','-nostdin',...args],{timeout,encoding:'utf8',maxBuffer:2000000});if(r.status!==0)throw Error('offline_video_stage_failed');}
const phase='sin(2*PI*T/4)';
const gaussian=(cx,cy,sx,sy)=>`exp(-pow((X-${cx})/${sx},2)-pow((Y-${cy})/${sy},2))`;
const directions={factory:'Cap lifts and seats while the two grippers articulate independently',economy:'Curved financial structures flex with circulating surface illumination',social:'Horn membranes vibrate, strands flex and signals travel through existing connections',trading:'Outer mechanical ring rocks, chart elements respond and the lens counter-rotates'};
function articulatedRecipe(id){
 // Fixed camera: all displacement fields are confined to source objects. No
 // whole-image scale, translation, generated scene or invented live activity.
 const t='2*PI*T/4',s=`sin(${t})`,c=`(1-cos(${t}))`;
 let dx='0',dy='0',light='1';
 if(id==='factory'){
  const cap=gaussian(238,91,55,28),left=gaussian(178,19,35,64),right=gaussian(375,110,65,35),body=gaussian(235,169,50,60);
  dx=`3*${s}*${left}-5*sin(${t}+0.6)*${right}+1.5*${s}*${body}`;
  dy=`9*${c}*${cap}+5*${c}*${left}-4*sin(${t}+0.6)*${right}`;
 }else if(id==='economy'){
  const structure=gaussian(240,135,170,133),stem=gaussian(244,120,40,80);
  dx=`6*sin(${t}+Y/75)*${structure}-4*${s}*${stem}`;
  dy=`4*cos(${t}+X/85)*${structure}`;
  light=`1+0.18*pow(0.5+0.5*cos(atan2(Y-135,X-240)-${t}),10)*${structure}`;
 }else if(id==='social'){
  const horn=gaussian(318,38,68,65),hub=gaussian(204,145,64,45),strands=gaussian(235,160,200,120);
  dx=`3*sin(2*${t})*${horn}+4*sin(${t}+Y/60)*${strands}`;
  dy=`3*cos(2*${t})*${horn}+3*sin(${t}+X/65)*${strands}-2*${s}*${hub}`;
  const edge='min(1,(abs(g(X+1,Y)-g(X-1,Y))+abs(g(X,Y+1)-g(X,Y-1)))/18)';
  light=`1+0.48*pow(0.5+0.5*cos(2*PI*(X/W+Y/H)-${t}),24)*${edge}*${strands}`;
 }else{
  const radius='sqrt(pow(X-240,2)+pow(Y-142,2))';
  const ring=`exp(-pow((${radius}-176)/15,4))`;
  const lens=gaussian(240,171,52,50),charts=gaussian(243,111,115,80);
  dx=`0.065*${s}*(Y-142)*${ring}-0.07*${s}*(Y-171)*${lens}`;
  dy=`-0.065*${s}*(X-240)*${ring}+0.07*${s}*(X-240)*${lens}+5*sin(${t}+X/35)*${charts}`;
  light=`1+0.18*pow(0.5+0.5*cos(atan2(Y-171,X-240)-${t}),12)*${lens}`;
 }
 return 'scale=480:480:flags=lanczos,crop=480:300:0:83,format=gbrp,geq='+['r','g','b'].map(channel=>`${channel}='clip(${channel}(clip(X+(${dx}),0,W-1),clip(Y+(${dy}),0,H-1))*(${light}),0,255)'`).join(':');
}
function recipe(id){
 let x='X',y='Y',light='1';
 if(id==='factory'){
  // Sub-pixel inverse warps around existing grippers; no new objects or surfaces.
  const left=gaussian(182,37,52,65),right=gaussian(390,115,70,45),cap=gaussian(238,90,57,38);
  x=`X+${phase}*(0.7*${left}-0.55*${right})`;
  y=`Y+${phase}*(0.65*${left}-0.45*${right}+0.35*${cap})`;
 }else if(id==='economy'){
  const ring=gaussian(240,130,205,155),angle=`0.007*${phase}*${ring}`;
  x=`X-(${angle})*(Y-130)`;y=`Y+(${angle})*(X-240)`;
  light=`1+0.025*pow(0.5+0.5*cos(atan2(Y-130,X-240)-2*PI*T/4),8)*${ring}`;
 }else if(id==='social'){
  // Traveling illumination follows only detail already present in the image.
  const edge='min(1,(abs(g(X+1,Y)-g(X-1,Y))+abs(g(X,Y+1)-g(X,Y-1)))/24)';
  light=`1+0.10*pow(0.5+0.5*cos(2*PI*(X/W+Y/H-T/4)),18)*${edge}`;
 }else if(id==='trading'){
  const lens=gaussian(240,171,66,66),angle=`0.005*${phase}*${lens}`;
  x=`X-(${angle})*(Y-171)`;y=`Y+(${angle})*(X-240)`;
  light=`1+0.055*pow(0.5+0.5*cos(atan2(Y-171,X-240)-2*PI*T/4),12)*${lens}`;
 }
 const channels=['r','g','b'].map(c=>`${c}='${c}(clip(${x},0,W-1),clip(${y},0,H-1))*(${light})'`).join(':');
 return `scale=480:480:flags=lanczos,crop=480:300:0:83,format=gbrp,geq=${channels}`;
}
const ffmpegVersion=spawnSync('ffmpeg',['-version'],{encoding:'utf8'}).stdout.split('\n')[0];
const records=only&&existsSync(resolve(output,'manifest.json'))?JSON.parse(readFileSync(resolve(output,'manifest.json'))).candidates.filter(c=>c.section!==only):[];
for(const id of sections.filter(id=>!only||id===only)){
 const source=resolve(root,`reference-web/public/assets/supa-${id}-960.webp`),master=resolve(work,`${id}-master.mkv`);
 const webm=resolve(output,`supa-${id}-loop.webm`),mp4=resolve(output,`supa-${id}-loop.mp4`);
 if(existsSync(webm)||existsSync(mp4))throw Error('existing_candidate_requires_separate_review');
 let filter=articulated?articulatedRecipe(id):recipe(id);
 if(refresh){
  // Preserve source composition while making motion visible at 200-pixel card width.
  filter=filter.replaceAll('0.7*','3.5*').replaceAll('0.55*','2.8*').replaceAll('0.65*','3.2*').replaceAll('0.45*','2.2*').replaceAll('0.35*','2.0*')
   .replaceAll('0.007*','0.024*').replaceAll('1+0.025*','1+0.07*')
   .replaceAll('1+0.10*','1+0.24*').replaceAll('0.005*','0.036*').replaceAll('1+0.055*','1+0.18*');
 }
 writeFileSync(resolve(work,`${id}-filter.txt`),filter+'\n');
 // Four seconds at 20 fps. The periodic equations return to the initial state
 // at t=4; do not duplicate the first frame at the end and introduce a hold.
 run(['-loop','1','-framerate','20','-i',source,'-vf',filter,'-t','4','-an','-map_metadata','-1','-threads','2','-c:v','ffv1','-y',master]);
 const sizing=refresh||articulated?['-vf','scale=320:200:flags=lanczos']:[];
 run(['-i',master,...sizing,'-an','-map_metadata','-1','-c:v','libvpx-vp9','-b:v','0','-crf','32','-row-mt','1','-threads','2','-pix_fmt','yuv420p',webm]);
 const mp4Encoding=articulated?['-qp','14','-preset','slow','-force_key_frames','0,3.95']:refresh&&id==='economy'?['-qp','11','-preset','veryslow','-force_key_frames','0,3.95']:['-qp','12','-preset','slow'];
 run(['-i',master,...sizing,'-an','-map_metadata','-1','-c:v','libx264',...mp4Encoding,'-bf','0','-x264-params','ipratio=1:pbratio=1:mbtree=0:aq-mode=0','-threads','2','-pix_fmt','yuv420p','-movflags','+faststart',mp4]);
 records.push({section:id,direction:articulated?directions[id]:undefined,mp4_encoding:mp4Encoding,source:`reference-web/public/assets/supa-${id}-960.webp`,source_sha256:hash(source),filter,webm_sha256:hash(webm),mp4_sha256:hash(mp4),width:refresh||articulated?320:480,height:refresh||articulated?200:300,fps:20,seconds:4,artwork_replaced:false,production_enabled:false});
 records.sort((a,b)=>sections.indexOf(a.section)-sections.indexOf(b.section));
 writeFileSync(resolve(output,'manifest.json'),JSON.stringify({status:articulated?'local_articulated_review':refresh?'local_playback_refresh':'unapproved_review_candidates',ffmpeg_version:ffmpegVersion,encoding:{webm:'VP9 CRF32',mp4:'H.264, no B frames, uniform I/P quantization, faststart; exact settings per clip'},method:articulated?'Fixed-camera source-derived 2D object articulation and traveling light; not generated 3D footage':'source-derived 2D motion and lighting; no generative video or 3D reconstruction',candidates:records},null,2)+'\n');
 console.log(JSON.stringify({section:id,prepared:true}));
}
