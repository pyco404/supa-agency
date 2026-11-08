/** Supa's read-only terminal views. All record text is inserted as text, never HTML. */
// Approved illustration catalog, not a requirement to illustrate every section.
// Placement belongs to individual public content rows; these are never runtime records or token images.
export const sectionArtwork = Object.freeze({
  home: { title: 'A shared orbit', alt: 'Sculpted AI cores joined by delicate ivory filaments in a dark space.' },
  agents: { title: 'Supa Agent', alt: 'Original Supa Agent concept portrait: an ivory robot with olive temples and green optical slits.' },
  intelligence: { title: 'Paths of thought', alt: 'An intricate sculptural network of folded reasoning pathways.' },
  memory: { title: 'What stays with us', alt: 'Layered ivory memory wafers inside a dark archival structure.' },
  economy: { title: 'Resources, with boundaries', alt: 'An abstract resource network of mechanical loops and fine conduits.' },
  social: { title: 'A signal worth sending', alt: 'Delicate signal instruments connected across a dark void.' },
  trading: { title: 'Observe before acting', alt: 'An abstract market-observation instrument with graphite fins and an optical lens.' },
  factory: { title: 'A purpose takes shape', alt: 'Precision manipulators assembling an ivory agent core: a concept, not an active factory.' },
  about: { title: 'Built with intent', alt: 'An exploded architectural sculpture of connected ivory plates and graphite frames.' },
});
// Add only reviewed 3–5 second, silent, seamless clips. Null slots deliberately
// make no network request; the current thumbnail is always the static fallback.
export const navigationMotion = Object.freeze({
  economy: Object.freeze({ webm: null, mp4: null, direction: 'Slow orbital rings and soft reflections' }),
  social: Object.freeze({ webm: null, mp4: null, direction: 'Delicate pulses through signal strands' }),
  trading: Object.freeze({ webm: null, mp4: null, direction: 'Slow optical rings and reflection drift' }),
  factory: Object.freeze({ webm: null, mp4: null, direction: 'Tiny, gentle assembly-arm movements' }),
});
export const navigation = [
  ['home','Home'],['agents','Agents'],['intelligence','Intelligence'],['memory','Memory'],['economy','Economy'],['social','Social'],['trading','Trading · planned'],['factory','Factory · planned'],['about','About'],
].map(([id,label])=>({id,label,path:id==='home'?'/':`/${id}`,kind:'section',media:navigationMotion[id],thumb:`/public/assets/supa-${id}-thumb.webp`}));
export const githubNotice = 'GitHub account private, will be open sourced soon.';
export const description = 'Independent AI agents that earn money, pay for infrastructure, improve their capabilities, and create other agents on Solana.';
// Reviewed editorial content only. No database, staging fixture or activity projection.
export const publicNavigation = navigation
  .map(item=>({...item,path:item.id==='home'?'/home':item.path}));
export function publicSectionView(id) {
  const rows=[{kind:'command',text:`supa ${id==='home'?'status':id}`},
    {kind:'notice',text:'Public exploration / read-only. Private agent records and controls require sign-in.'}];
  const add=(text,kind='text')=>rows.push({text,kind});
  if(id==='home') {
    add('Supa Agency','heading');add('One agent. One purpose. One token.');
    add('Explore purposeful AI agents on Solana. Reasoning, memory, work and learning are the foundation of the platform vision.');
    rows.push({kind:'artwork',id:'home'});
    add('Supa Agent','heading');add('Supa is our flagship autonomous AI personality: curious, witty and occasionally sarcastic. This introduction is not a live activity report.');
    rows.push({kind:'navigate',id:'factory',text:'Create Agent →'});
    rows.push({kind:'navigate',id:'agents',text:'Discover public agents →'});
    add('Agent creation and management require authenticated, explicitly authorized access. Public browsing cannot start an agent.');
  } else if(id==='agents') {
    add('Public agents','heading');add('No public agent profiles are available yet.','notice');
    add('Only creator-approved profiles with verified public information will appear here. Private agents, goals, memories, wallets and activity are not included.');
    add('Supa Agent remains our flagship personality. No live profile or achievements are claimed by this empty directory.');
    rows.push({kind:'artwork',id:'agents'});
    add('Agent creation is Coming Soon. Management requires sign-in and separate authorization.');
    rows.push({kind:'navigate',id:'factory',text:'Create Agent →'});
  } else if(id==='intelligence') {
    add('Intelligence','heading');
    add('Reasoning, purpose and bounded goal execution are part of Supa Agent’s foundation. Viewing this overview does not run inference.');
    add('No creator-approved reasoning reports are available for public viewing. Private goals, prompts and execution records are not shown.','notice');
    add('Run agent / Coming Soon','disabled');
  } else if(id==='memory') {
    add('Memory','heading');
    add('Persistent memory supports learning and continuity. Memories remain private unless their creator explicitly approves a public disclosure.');
    add('No approved public memory summaries are available. Private memories are never loaded by this page.','notice');
    add('Manage memories / Unavailable in public exploration','disabled');
  } else if(id==='economy') {
    add('Economy','heading');
    add('The platform vision includes authorized earnings and operating expenses. Operational accounting is separate from token markets; token ownership grants no revenue rights.');
    add('No verified public economic reports are available. Balances, wallets and financial records are private.','notice');
    add('Financial execution / Disabled','disabled');
  } else if(id==='social') {
    add('Social','heading');
    add('Supa’s ecosystem voice: curious, witty and occasionally sarcastic. Planned reports must separate verified facts from commentary and respect creator consent.');
    add('No approved public activity or social drafts are available. This page does not read private events or publish posts.','notice');
    rows.push({kind:'link',text:'Official X / @supa_agency',href:'https://x.com/supa_agency'});
    add('Autonomous X publishing / Disabled','disabled');
  } else if(id==='trading') {
    add('Trading / Coming Soon','heading');
    add('Bounded trading is a planned, separately authorized capability. Public exploration cannot place orders or sign transactions.');
    add('No public trades, performance or liquidity reports are available. No simulated returns are presented as real results.','notice');
    add('Trading / Disabled','disabled');
  } else if(id==='token-launch') {
    add('Token Launch / Coming Soon','heading');
    add('Pump.fun is the planned launch integration. Agent creation, purpose qualification and explicitly signed token launches are separate operations.');
    add('The platform token $SUPA is planned, not live. It is distinct from individual agent tokens. Each qualifying agent may have one original permanent official mint after verified launch confirmation.');
    add('No verified public token launches are listed. Token ownership grants no wallet control, agent permissions or automatic revenue rights.','notice');
    add('Launch token / Disabled','disabled');
  } else if(id==='factory') {
    const synthetic = typeof document !== 'undefined' && document.documentElement?.dataset.syntheticFactory === 'true';
    add('Factory / Create Agents','heading');
    if (synthetic) rows.push({kind:'link',text:'Create Agent / synthetic sign-in',href:'/sign-in'});
    add(synthetic?'Invited synthetic accounts can sign in to save a private paused agent. Execution and spending remain disabled.':'Build an agent around a purpose. Creation is Coming Soon; this preview does not save information or activate an agent.','notice');
    add('Create an agent without launching a token. Supa Agency is the platform; Supa Agent remains its flagship personality.');
    add('1. Create Agent','heading');
    add(synthetic?'Name, purpose, description and personality are collected only after sign-in and private-storage consent.':'Name, purpose, description and personality. No details are collected in this preview.');
    rows.push({kind:'artwork',id:'factory'});
    add('Create Agent / Coming Soon','disabled');
    add('2. Configure','heading');
    add('Choose capabilities, goals, permissions, autonomy level and spending limits. Default to paused with execution disabled; changes require explicit approval.');
    add('Configure / Coming Soon','disabled');
    add('3. Purpose Qualification','heading');
    add('Validate the purpose, confirm creator consent and obtain required approvals. Purpose qualification does not authorize execution or token launching.');
    rows.push({kind:'artwork',id:'intelligence'});
    add('Purpose Qualification / Coming Soon','disabled');
    add('4. Agent Token / Optional','heading');
    add('Skip this step and keep your agent token-free. Pump.fun is the planned integration; any future launch requires separate authorization, wallet signatures and verified confirmation.');
    add('The planned platform token $SUPA is separate from individual agent tokens. One original confirmed mint binds permanently to one qualifying agent; no silent replacement or reuse across agents.');
    add('Exceptional successors require separate authorization, public disclosure and a link to the original mint. They do not transfer balances, rights or claims automatically. Token holders gain no automatic wallet control, permissions or revenue rights.');
    add('Launch agent token / Unavailable','disabled');
    add('5. My Agents','heading');
    add(synthetic?'Sign in to view your saved private agents and configuration. Lifecycle execution remains unavailable.':'Authenticated owner management is Coming Soon. Private memories, credentials, permissions, financial records and lifecycle controls remain protected.');
    rows.push({kind:'link',text:'My Agents / sign-in required',href:'/my-agents'});
    rows.push({kind:'navigate',id:'agents',text:'Discover public agents →'});
  } else if(id==='about') {
    add('About Supa Agency','heading');add('One agent. One purpose. One token.');
    add('Supa Agency is the platform. Supa Agent is its flagship autonomous AI agent, with reasoning, memory and a distinctive personality.');
    add('Our vision: purposeful agents that learn, collaborate and sustain their work under explicit permissions and operating budgets.');
    add('The platform token $SUPA is planned, not live. Each qualifying agent may have one permanent official Solana token, distinct from $SUPA. Purpose review and token launch are separate operations.');
    add('Pump.fun integration is planned. Token ownership grants no wallet control, agent permissions or automatic revenue rights.');
    add('Token launches, trading, wallet signing, paid inference and autonomous X publishing are disabled. Public exploration performs none of these actions.');
    rows.push({kind:'link',text:'Official X / @supa_agency',href:'https://x.com/supa_agency'});
    rows.push({kind:'github',text:'Source / github.com'});
  } else add('This view is not part of public exploration.','notice');
  rows.push({kind:'link',text:'Authorized operator sign-in',href:'/operator'});
  return rows;
}
export const units = (value, decimals=6) => {
  if (!Number.isSafeInteger(value)) return 'unavailable';
  const amount=BigInt(value),scale=10n**BigInt(decimals),abs=amount<0n?-amount:amount;
  const fraction=(abs%scale).toString().padStart(decimals,'0').replace(/0+$/,'');
  return `${amount<0n?'−':''}${abs/scale}${fraction?'.'+fraction:''}`;
};
export async function read(path) {
  if (globalThis.document?.documentElement.dataset.publicLanding === 'true') return {error:'Private records require sign-in.'};
  try {
    const response=await fetch(path,{cache:'no-store',credentials:'same-origin',signal:AbortSignal.timeout(6500)});
    if(!response.ok) return {error:response.status===401?'Operator sign-in required. Private records are not public.':response.status===404?'No saved resource is available.':'Saved records are unavailable. Check the local backend and reload.'};
    return {data:await response.json()};
  } catch { return {error:'Disconnected. Saved records could not be retrieved. Reload when the local backend is available.'}; }
}
export async function overview() {
  const [status,agents]=await Promise.all([read('/api/status'),read('/api/agents')]);
  return {status,agents,retrievedAt:new Date().toISOString()};
}
export async function sectionView(id, state, agentId) {
  const rows=[];
  const add=(text,kind='text')=>rows.push({text,kind});
  const record=(label,value)=>add(`${label}  ${value ?? 'unavailable'}`);
  const empty=label=>add(`No saved ${label} were returned.`);
  const load=async name=>{
    const result=await read(`/api/agents/${agentId}${name?'/'+name:''}`);
    if(result.error)add(`${name}: ${result.error}`,'notice');
    return result.data;
  };
  const agents=state.agents.data;
  add(`supa ${id==='home'?'status':id}`,'command');
  let epoch=state.status.data?.epoch;
  let operator,launchMode;
  if(id==='intelligence') {
    const response=await read('/api/operator-status'); operator=response.data;
    epoch??=operator?.epoch;
    launchMode=epoch?undefined:(await read('/api/launch')).data;
    if(launchMode?.mode==='disposable_offline'){add('Disposable local development','heading');record('Your role',launchMode.role);record('Test sign-in expires',launchMode.expires_at);add('Offline deterministic reasoning. Each session needs your finite authorization below. Primary and historical evaluations are not connected.','notice');}
    else {add('Operator / verified service metadata','heading');
    if(!operator)add(response.error??'Operator status unavailable. Execution eligibility is unverified.','notice');
    else {
      record('Credential',operator.credential.status);record('Your role',operator.role);
      record('Scoped authorization',`${operator.authorization.status} / ${operator.authorization.scope??'none'}`);
      record('Permit expires',operator.authorization.expires_at??'No admitted permit');
      record('Scope target',operator.authorization.agent_id??'No target');
      record('Worker',operator.worker.status.replaceAll('_',' '));
      record('Execution',operator.worker.execution_running?'Verified active lease and recent heartbeat':'No verified running execution');
      record('Offline control eligibility',operator.eligibility.activate_offline?'Scoped permission; fresh finite agent consent required':'Not authorized');
      add('Paid inference and financial operations remain disabled. A permit is not evidence of running intelligence.','notice');
    }
    }
  }
  add(launchMode?.mode==='disposable_offline'?'Isolated test database / simulated capital only.':epoch?`Development epoch / ${epoch.id}. ${epoch.state==='active_read_only'?'Active, read-only':'Prepared, read-only'}; historical database unrecovered.`:state.status.data?'Epoch unidentified. Historical recovery is not verified.':'Epoch unavailable while the backend is disconnected.','notice');
  if(id==='home') {
    add('Supa Agency / SUPA','heading');add('One agent. One purpose. One token.','heading');
    add('Persistent AI agents with bounded reasoning, memory and economic awareness. Explore saved records; this site cannot start paid operations.');
    rows.push({kind:'navigate',id:'factory',text:'Create Agent →'});
    rows.push({kind:'navigate',id:'agents',text:'Explore saved agents →'});
    if(state.agents.error)add(state.agents.error,'notice');
    record('API',state.status.data?.status ?? 'disconnected');record('Economic mode','simulation');
    record('Agents',agents?.length);record('Retrieved',state.retrievedAt);
    add('Choose an identity in Agents, then explore Intelligence, Memory, Economy or Social. Private records require operator sign-in.','notice');
    add('Platform vision');add(description);add('Earning, infrastructure payments, capability improvement and agent creation describe the vision; autonomous real-money services are not enabled.');
  } else if(id==='agents') {
    rows.push({kind:'navigate',id:'factory',text:'Create Agent →'});
    rows.push({kind:'link',text:'My Agents / sign-in required',href:'/my-agents'});
    add('Existing agents','heading');
    if(!agents) add(state.agents.error,'notice');
    else if(!agents.length)empty('agents');
    else for(const agent of agents) rows.push({kind:'agent',agent,text:`${agent.name} / ${agent.lifecycle}`});
    const offers=epoch?{data:{services:[]}}:await read('/api/services');
    add('Available services','heading');
    if(offers.data?.services.length)for(const offer of offers.data.services)add(`${offer.spec.name} / ${offer.spec.kind} / ${units(offer.spec.price_atoms)} per job. Supplied-input analysis; requires evaluate_goal permission.`);else add(offers.error??'No agent services have been offered.');
    add('Choose an agent to inspect its saved records. Network and activity counts appear only when authorized records are available.');
  } else if(id==='factory') {
    return publicSectionView('factory');
  } else if(['trading','token-launch'].includes(id)) {
    add('Planned / non-executable','heading');
    add(id==='trading'?'Trading is not enabled. Planned policy: a strict maximum of $5 per trade, subject to independent authorization and financial controls.':id==='token-launch'?'Pump.fun token launching is planned. No token has been launched through this interface.':'Human and agent-created agents are planned. Creation requires explicit authorization; this interface cannot create agents.');
    add('No action on this page signs, transfers, trades, publishes or launches anything.');
  } else if(id==='about') {
    add('Supa Agency / SUPA','heading');add('One agent. One purpose. One token.');add('Vision: '+description);
    add('Current foundation: six-crate Rust runtime, PostgreSQL persistence, metered LLM abstraction, permissioned tools, simulated economics and read-only Devnet observations.');
    add('The model proposes actions. Application policy decides what is permitted. Personality never grants financial permissions.');
    add('One associated Solana token per qualifying agent is planned. Creation, purpose review and launch are separate operations. Token ownership grants no agent control.');
    add('Mainnet spending, trading, publishing, token launches and child-agent creation remain disabled. The planned Supa Agency token (SUPA) on Solana is not live. Launching other tokens is a separate planned platform capability.');
    add('Domain purchase is owner-confirmed; deployment remains pending. Official X account; autonomous publishing remains disabled.');
    add('Official domain / supaagency.xyz — purchased; official-domain launch pending.');
    rows.push({kind:'link',text:'Official X / @supa_agency',href:'https://x.com/supa_agency'});
    rows.push({kind:'github',text:'Source / github.com'});
  } else if(!agentId) { add('Select an agent in Agents to explore saved records.','notice'); }
  else {
    const agent=agents?.find(a=>a.id===agentId);
    add(`${agent?.name ?? 'Selected agent'} / ${agent?.lifecycle ?? 'status unavailable'}`,'heading');
    record('Identity',agentId);
    if(id==='intelligence') {
      add('Offline cycles and controlled live model sessions have separate histories and accounting. Viewing records never runs inference.','notice');
      const [profile,wallet,goals,cycles,actions,activity]=await Promise.all(['','wallet','goals','cycles','actions','activity'].map(name=>name==='wallet'&&launchMode?.mode==='disposable_offline'?Promise.resolve(null):load(name)));
      const [schedule,health,history,access]=await Promise.all(['schedule','execution-health','authorizations','access'].map(resource=>read(`/api/agents/${agentId}/${resource}`)));
      if(access.data)record('Your role',access.data.role);
      const live=await read(`/api/agents/${agentId}/live`);
      if(access.data?.role==='owner'){const work=await read(`/api/agents/${agentId}/live-work`);if(work.data)rows.push({kind:'livework',view:work.data,agentId});else add(work.error??'Work evaluation unavailable.','notice');}
      if(!epoch && live.data && access.data)rows.push({kind:'live',view:live.data,role:access.data.role,agentId});
      else add('Controlled live inference unavailable. No paid requests are enabled by viewing this page.','notice');
      if(health.data){record('Worker health',health.data.status.replaceAll('_',' '));record('Worker process',health.data.worker_enabled===false?'Scheduling disabled in observed process':health.data.worker_enabled===true?'Scheduling enabled in observed process':'Configuration not reported');record('Execution',health.data.execution_running?'Measured active lease and recent heartbeat':'No verified running execution');record('Heartbeat observed',health.data.heartbeat_at??'No heartbeat recorded');record('Lease expires',health.data.lease_until??'No active lease');add('Authorization permits work; it does not prove a worker is running.','notice');}
      else add('Worker health is unavailable. No running execution can be verified.','notice');
      if(schedule.data && goals && access.data)rows.push({kind:'schedule',status:schedule.data,goals,role:epoch&&!operator?.eligibility.activate_offline?'viewer':access.data.role});
      if(history.data?.length)rows.push({kind:'detail',text:'Authorization approval history',body:history.data.map(a=>`Authorization ${a.id} / ${a.legacy?'legacy: approver not recorded':a.approved_by??'approver unavailable'}\nApproved ${a.approved_at??'time not recorded'} / expires ${a.expires_at} / closed ${a.closed_at??'current epoch'}\nGoals ${a.policy.goal_ids.join(', ')}\nLimit ${a.policy.max_cycles} cycles / ${a.policy.simulated_budget_microusd} simulated microUSD / interval ${a.policy.interval_seconds}s\nConsumed ${a.cycles_started} cycles / ${a.reserved_microusd} simulated microUSD`).join('\n\n')});
      if(profile){add('Agent profile','heading');record('Personality',profile.agent.personality.voice);record('Lifecycle',profile.agent.lifecycle);record('Created',profile.agent.created_at);record('Verified activity records',activity?.length);record('Wallet network',wallet?.network ?? 'No saved wallet identity');record('Wallet address',wallet?.address ?? 'unavailable');}
      if(goals){add('Goals','heading');if(!goals.length)empty('goals');goals.forEach(g=>add(`${g.completed?'complete':'open'} / priority ${g.priority} / suggested progress ${g.suggested_progress_bps/100}%\n${g.description}\nref ${g.id}`));}
      if(cycles){add(`Saved reasoning cycles / ${cycles.length}`,'heading');cycles.forEach(c=>add(`${c.status} / ${c.started_at}\ncycle ${c.id}\nfinished ${c.finished_at ?? 'not recorded'}${c.failure_code?' / failure '+c.failure_code:''}`));}
      const workRecords=await load('economy');
      if(workRecords?.jobs){
        const jobs=workRecords.jobs.filter(job=>job.output?.work||job.state==='executing');
        add('Bounded supplied-input work','heading');if(!jobs.length)empty('bounded task results');
        for(const job of jobs.slice(0,8)){
          const work=job.output?.work;add(`${job.spec.name} / ${job.state} / ${job.network}\njob ${job.id}\n${work?`${work.status} / ${work.steps?.length??0} recorded steps / ${work.evidence?.length??0} supplied references`:'An execution lease was claimed; completion is not verified.'}`);
          if(work)rows.push({kind:'detail',text:'Task result and evidence',body:JSON.stringify({work,operator_memory_references:job.assessment?.private_memory_references??[]},null,2)});
        }
        add('Inspect Economy for scoped task controls and full reports. Offline results are not live model runs.','notice');
      }
      if(actions){add(`Tool actions / ${actions.length}`,'heading');actions.forEach(a=>add(`${a.proposal.tool} / authorization ${a.authorization_outcome} / ${a.status}\ncycle ${a.cycle_id}${a.failure_code?' / failure '+a.failure_code:''}`));}
      if(activity){add(`Saved activity / ${activity.length}`,'heading');activity.forEach(e=>add(`${e.simulated?'Simulated':'Recorded'} / ${e.kind} / ${e.code}\n${e.created_at} / event ${e.id}`));}
    } else if(id==='memory') {
      add('Operator-only learning records. Hypotheses are not established facts.','notice');
      const memories=await load('memory');
      if(memories){if(!memories.length)empty('memories');memories.forEach(m=>add(`${m.kind==='verified_observation'?'Verified observation':'Hypothesis / unverified'}\n${m.lesson}\n${m.created_at}\nmemory ${m.id}\nevents ${m.source_event_ids.join(', ') || 'none'}\ncycle ${m.cycle_id ?? 'not recorded'}`));}
    } else if(id==='economy') {
      const economic=await load('economy');
      if(economic)rows.push({kind:'economy',view:economic,agentId});
      const work=await read(`/api/agents/${agentId}/live-work`);if(work.data)rows.push({kind:'livework',view:work.data,agentId});
      const [financials,ledger,wallet,balances,treasury,payments]=await Promise.all(['financials','simulated-ledger','wallet','wallet/balances','wallet/treasury','wallet/payments'].map(load));
      add('Simulated ledger / hypothetical USD','heading');
      if(financials){for(const [key,label]of [['balance_microusd','Balance'],['revenue_microusd','Revenue'],['inference_microusd','Inference expense'],['operating_microusd','Operating expense']])record(`${label} / simulated USD`,units(financials[key]));record('Runway',financials.runway_days===null?'Unavailable: a usable recurring-cost estimate is not established.':`${financials.runway_days} days (estimate)`);rows.push({kind:'detail',text:'Runway calculation',body:financials.runway_assumption});}
      if(ledger){if(!ledger.length)empty('simulated transactions');ledger.forEach(e=>add(`Simulated / ${e.kind} / ${units(e.amount_microusd)} USD\n${e.created_at} / ${e.id}`));}
      add('Solana Devnet / test assets only','heading');
      if(wallet)record('Wallet / Devnet',wallet.address);
      if(balances){record('Evidence',balances.evidence_source==='configured_rpc'?'Configured RPC observation (saved)':'Mock RPC / test fixture');record('SOL / Devnet',units(balances.sol_lamports,9));record('USDC / Devnet',units(balances.usdc_atoms));record('Observed slot',balances.slot);}
      else if(balances===null)add('No saved wallet balance observation.');
      if(treasury){record('Observation time',treasury.observed_at);record('Observation freshness',treasury.fresh?'fresh per backend':'stale');record('Reconciled',treasury.reconciled?'yes':'no');record('Verified incoming USDC / Devnet',units(treasury.verified_incoming_usdc_atoms));}
      if(payments){add('Recorded confirmed receipts / Devnet','heading');if(!payments.length)empty('receipts');payments.forEach(p=>add(`Devnet / ${p.evidence_source} / ${units(p.amount_atoms)} USDC\nmint ${p.mint}\nsignature ${p.signature}\nslot ${p.slot} / confirmed receipt record`));}
      add('Mainnet / unavailable. Devnet assets and simulated USD are separate and have no real-money value here. Signing is disabled.','notice');
    } else if(id==='social') {
      const [social,drafts]=await Promise.all(['social','drafts'].map(load));
      add('Persisted drafts / never published by this website','heading');
      add('Original stored wording follows. Financial statements concern simulation unless the cited record explicitly identifies Devnet test assets. Provider attribution is unavailable in this read API.','notice');
      if(drafts){if(!drafts.length)empty('drafts');drafts.forEach(d=>rows.push({kind:'draft',text:d.data.text ?? 'No draft text / abstention',meta:`Unpublished / ${d.data.style ?? 'style not recorded'} / ${d.created_at}\nevents ${d.data.event_ids.join(', ') || 'none'}\nmemories ${d.data.memory_ids?.join(', ') || 'none'}\ndraft ${d.id}`}));}
      if(social){add('Review history','heading');if(!social.length)empty('review records');social.forEach(s=>add(`${s.disposition} / ${s.data.choice ?? 'choice not recorded'} / ${s.data.style ?? 'style not recorded'}\n${s.data.reason ?? s.failure_code ?? 'No review reason saved'}\nevents ${s.data.event_ids.join(', ') || 'none'}\n${s.created_at}`));}
      add('X publishing / disabled. No publication history endpoint is available.');
    }
    record('Retrieved',new Date().toISOString());
  }
  if(id==='intelligence' && epoch && ['prepared','active_read_only'].includes(epoch.state))add(epoch.state==='prepared'?'Prepared epoch: read-only by default. Only a verified, finite, operation-specific server permit can authorize offline writes.':'Active read-only epoch: records are available; worker execution, ownership changes, authorizations and paid inference remain disabled.','notice');
  if(id==='intelligence' && !launchMode && (!epoch || !['prepared','active_read_only'].includes(epoch.state))){
    const workflow=await read('/api/autonomy');
    if(workflow.data)rows.splice(1,0,{kind:'autonomy',policy:workflow.data});
    else add('Offline autonomy controls unavailable. Operator sign-in and a backend supporting this workflow are required.','notice');
  }
  return rows;
}
