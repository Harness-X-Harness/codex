"""Synthetic loopback SSE; never contacts an upstream service."""
import json
def sse(body,n):
 out=[]
 def emit(e):out.append('data: '+json.dumps(e)+'\n\n')
 rid='offline-response-'+str(n);emit({'type':'response.created','response':{'id':rid}})
 if n==1:
  tool=next(t['name'] for t in body['tools'] if t.get('description','').endswith('Return a fresh token for this history probe.'))
  reason={'type':'reasoning','id':'offline-reason','summary':[],'encrypted_content':'opaque-offline-cipher','content':[{'type':'reasoning_text','text':'OFFLINE_PRIVATE_CANARY'}]}
  for typ in ['added','done']:emit({'type':'response.output_item.'+typ,'output_index':0,'item':reason})
  call={'type':'function_call','id':'offline-call-item','call_id':'offline-call','name':tool,'arguments':''}
  emit({'type':'response.output_item.added','output_index':1,'item':call});emit({'type':'response.function_call_arguments.delta','output_index':1,'delta':'{}'})
  call=call|{'arguments':'{}','status':'completed'};emit({'type':'response.output_item.done','output_index':1,'item':call})
 else:
  token=next(x['output'] for x in body['input'] if x.get('type')=='function_call_output')
  text=json.dumps({'token':token},separators=(',',':'));msg={'type':'message','id':rid+'-msg','role':'assistant','phase':'final_answer','content':[]}
  emit({'type':'response.output_item.added','output_index':0,'item':msg});emit({'type':'response.output_text.delta','output_index':0,'delta':text});msg=msg|{'status':'completed','content':[{'type':'output_text','text':text}]};emit({'type':'response.output_item.done','output_index':0,'item':msg})
 emit({'type':'response.completed','response':{'id':rid}});return ''.join(out).encode()
