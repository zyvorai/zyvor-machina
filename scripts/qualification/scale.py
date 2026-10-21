#!/usr/bin/env python3
import argparse, concurrent.futures, json, statistics, time, urllib.request
p=argparse.ArgumentParser(); p.add_argument('--url', required=True); p.add_argument('--token', required=True); p.add_argument('--requests', type=int, default=1000); p.add_argument('--concurrency', type=int, default=32); a=p.parse_args()
headers={'Authorization':f'Bearer {a.token}'}
def once(_):
    t=time.perf_counter(); req=urllib.request.Request(a.url.rstrip('/')+'/api/v1/vms?limit=500',headers=headers)
    with urllib.request.urlopen(req,timeout=20) as r: r.read()
    return (time.perf_counter()-t)*1000
with concurrent.futures.ThreadPoolExecutor(max_workers=a.concurrency) as ex: lat=list(ex.map(once, range(a.requests)))
lat.sort(); pct=lambda q: lat[min(len(lat)-1,int(len(lat)*q))]
print(json.dumps({'requests':len(lat),'concurrency':a.concurrency,'p50_ms':pct(.50),'p95_ms':pct(.95),'p99_ms':pct(.99),'mean_ms':statistics.mean(lat)},indent=2))
