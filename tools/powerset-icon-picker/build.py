"""Builds powerset-icon-picker.html: every primary/secondary set with its power icons, to pick
each set's icon by eye. Reads and exports hand-data/powerset-icons.json. Run from anywhere:
python3 tools/powerset-icon-picker/build.py, then open the HTML it writes next to itself."""
import json,glob,os,base64
here=os.path.dirname(os.path.abspath(__file__))
root=os.path.dirname(os.path.dirname(here))
out=os.path.join(here,'powerset-icon-picker.html')
img=f'{root}/crates/app/assets/img/powers'
have={f.lower():f for f in os.listdir(img)}
icons={}
for ds in ['homecoming','rebirth','thunderspy','brainstorm']:
    for f in glob.glob(f'{root}/contract/{ds}/powersets/*/*/*.json'):
        d=json.load(open(f))
        for p in d.get('powers',[]):
            ic=(p.get('icon') or '').lower()
            m=icons.setdefault(d['name'],{})
            if p['name'] not in m or (m[p['name']] not in have and ic in have): m[p['name']]=ic
picks=json.load(open(f'{root}/hand-data/powerset-icons.json'))
uris={}; sets=[]
for name,v in picks.items():
    ch=[]
    for p in v['choices']:
        ic=icons[name][p]
        if ic not in uris:
            uris[ic]='data:image/png;base64,'+base64.b64encode(open(f'{img}/{have[ic]}','rb').read()).decode()
        ch.append([p,ic])
    sets.append({'name':name,'pick':v['pick'],'choices':ch,'archetypes':v['archetypes']})
html=open(os.path.join(here,'picker_template.html')).read()
html=html.replace('/*DATA*/null',json.dumps({'sets':sets,'icons':uris},separators=(',',':')))
open(out,'w').write(html)
print(len(sets),len(uris),os.path.getsize(out)//1024,'KB')
