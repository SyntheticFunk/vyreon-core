import gzip, json, sys, collections
IDN = {"result_id","history_sha256","snapshot_cutoff","body_sha256","history_index_sha256","product_stream_root_sha256","current_chain_manifest_sha256","endpoint_index_sha256","endpoint_set_sha256","source_capture_set_sha256","revision_id"}
LEFT = set(sys.argv[3].split(","))
def load(p): return {(r["measurement_id"], r["object_id"]): r for r in map(json.loads, gzip.open(p, "rt"))}
def leaves(v, path=()):
    if isinstance(v, dict):
        if "role_id" in v and isinstance(v.get("role_id"), str): path = path[:-1] + (v["role_id"],)
        for k, x in v.items():
            if k not in IDN: yield from leaves(x, path + (k,))
    elif isinstance(v, list):
        for i, x in enumerate(v): yield from leaves(x, path + (i,))
    else: yield path, v
A, B = load(sys.argv[1]), load(sys.argv[2])
left = collections.Counter(); leftreasons = collections.Counter(); leftsame=[]
other = collections.defaultdict(lambda: [0, 0.0, None, 0]); 
for k in sorted(A):
    la, lb = dict(leaves(json.loads(A[k]["body_text"]))), dict(leaves(json.loads(B[k]["body_text"])))
    if k[1] in LEFT:
        if la == lb: leftsame.append(k[0])
        else:
            left[k[1]] += 1
            for p, v in lb.items():
                if p and p[-1] == "reason" and v and la.get(p) != v: leftreasons[(k[0].split("-")[0], v)] += 1
        continue
    if la == lb: continue
    o = other[k[0]]; o[0] += 1
    for p in set(la) | set(lb):
        x, y = la.get(p), lb.get(p)
        if x == y: continue
        if isinstance(x, (int, float)) and isinstance(y, (int, float)) and not isinstance(x, bool) and not isinstance(y, bool):
            rel = abs(x - y) / max(abs(x), abs(y), 1e-300)
            if rel > o[1]: o[1], o[2] = rel, (k[1], str(p[-3:]), x, y)
        else: o[3] += 1
print("left-out assets: differing slots", dict(left), "unchanged:", sorted(set(leftsame)))
print("new reasons on left-out assets (family, reason):"); [print("  ", n, k) for k, n in leftreasons.most_common(25)]
print("other assets, by measurement: slots differing, worst numeric rel diff, non-numeric leaf diffs")
for m, o in sorted(other.items(), key=lambda t: -t[1][1]): print(f"  {m:28s} {o[0]:4d}  {o[1]:.2e}  nonnum {o[3]:5d}  {str(o[2])[:150]}")
print("total other slots differing:", sum(o[0] for o in other.values()))
