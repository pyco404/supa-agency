"""Read-only logical recovery comparison queries; no database access on import."""
import json

def quote(v): return "'"+v.replace("'","''")+"'"
def ident(v): return '"'+v.replace('"','""')+'"'
def stream(text):
 d=json.JSONDecoder();r=text[text.find('{'):];v=[]
 while r.strip():
  r=r.lstrip();o,n=d.raw_decode(r);v.append(o);r=r[n:]
 return v

def sql(tables, temporary=(), stable_health=False):
 base="n.nspname IN ('public','epoch_guard')"
 exclude=','.join(quote(x) for x in temporary) or "''"
 objects=f"""SELECT 'schema:'||n.nspname AS identity,n.nspowner AS owner,n.nspacl AS acl,'n'::\"char\" AS kind FROM pg_namespace n WHERE {base}
 UNION ALL SELECT 'relation:'||n.nspname||'.'||c.relname,c.relowner,c.relacl,CASE WHEN c.relkind='S' THEN 's' ELSE 'r' END::\"char\" FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE {base} AND c.relkind IN ('r','p','S','v','m')
 UNION ALL SELECT 'function:'||p.oid::regprocedure::text,p.proowner,p.proacl,'f'::\"char\" FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE {base} AND p.prokind IN ('f','p')"""
 queries={
 'columns':"SELECT table_schema,table_name,column_name,ordinal_position,data_type,is_nullable,column_default FROM information_schema.columns WHERE table_schema IN ('public','epoch_guard') ORDER BY table_schema,table_name,ordinal_position",
 'constraints':f"SELECT n.nspname AS schema,c.conname AS name,c.conrelid::regclass::text AS relation,pg_get_constraintdef(c.oid) AS definition,c.convalidated FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE {base} ORDER BY schema,name,relation",
 'indexes':"SELECT schemaname,indexname,indexdef FROM pg_indexes WHERE schemaname IN ('public','epoch_guard') ORDER BY schemaname,indexname",
 'sequences':"SELECT * FROM pg_sequences WHERE schemaname IN ('public','epoch_guard') ORDER BY schemaname,sequencename",
 'functions':f"SELECT p.oid::regprocedure::text AS identity,pg_get_userbyid(p.proowner) AS owner,pg_get_functiondef(p.oid) AS definition FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE {base} AND p.prokind IN ('f','p') ORDER BY identity",
 'triggers':f"SELECT n.nspname||'.'||c.relname AS relation,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid) AS definition FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE {base} AND NOT t.tgisinternal ORDER BY relation,t.tgname",
 'migration_history':"SELECT version,encode(checksum,'hex') AS checksum,success FROM public._sqlx_migrations ORDER BY version",
 'acl':f"""WITH objects AS ({objects}) SELECT o.identity,pg_get_userbyid(o.owner) AS owner,(SELECT coalesce(json_agg(q ORDER BY q.grantor,q.grantee,q.privilege_type,q.is_grantable),'[]'::json) FROM (SELECT pg_get_userbyid(x.grantor) AS grantor,CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END AS grantee,x.privilege_type,x.is_grantable FROM aclexplode(coalesce(o.acl,acldefault(o.kind,o.owner))) x WHERE coalesce(pg_get_userbyid(x.grantee),'PUBLIC') NOT IN ({exclude})) q) AS grants FROM objects o ORDER BY o.identity"""
 }
 out=''
 for k,q in queries.items(): out+=f"SELECT json_build_object('key',{quote(k)},'rows',(SELECT coalesce(json_agg(q),'[]'::json) FROM ({q}) q));\n"
 for name in tables:
  relation='.'.join(ident(x) for x in name.split('.'))
  row="to_jsonb(t)-'heartbeat_at'" if stable_health and name=='public.worker_health' else 'to_jsonb(t)'
  out+=f"SELECT json_build_object('key','data','name',{quote(name)},'count',count(*),'sha256',encode(sha256(convert_to(coalesce(string_agg(({row})::text,E'\\n' ORDER BY ({row})::text),''),'UTF8')),'hex')) FROM {relation} t;\n"
 return out
