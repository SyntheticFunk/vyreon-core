#!/usr/bin/env bash
# One screen of the run's health, read-only: what is running, what is done, and every oddity
# flagged with "!!". Run at every check during a build (and whenever in doubt).
# Needs KUBECONFIG and VYREON_DOKS_CLUSTER; the DO token as for do.sh. Prints no secret.
set -uo pipefail
S=$(cd "$(dirname "$0")" && pwd); C=${VYREON_DOKS_CLUSTER:?set VYREON_DOKS_CLUSTER}
CEILING=${VYREON_SPEND_CEILING:-500}
NS=vyreon
core() { kubectl -n $NS exec -c core deploy/vyreon-core -- vyreon-core "$@" 2>/dev/null; }
sql() { kubectl -n $NS exec vyreon-pg-1 -c postgres -- psql -X -A -t -F ' ' -d vyreon -c "$1" 2>/dev/null; }

# MT is the owner's clock, UTC−6 all year (C-072; Etc/GMT+6 is UTC−6, the POSIX sign inverted)
echo "== $(date -u '+%Y-%m-%d %H:%M UTC') ($(TZ=Etc/GMT+6 date '+%H:%M MT'))"

# money: DigitalOcean's own month-to-date usage. C-068 (audit opsurface ops.md 11): an error answer
# (do.sh exits 1 on anything but 2xx) or an answer without the figure is "unreadable", never $0.00
if BAL=$("$S/do.sh" GET /v2/customers/my/balance 2>&1); then
  echo "$BAL" | python3 -c "
import json,sys
b=json.load(sys.stdin); u=b.get('month_to_date_usage')
if u in (None, ''): print('!! spend: unreadable (no month_to_date_usage in the answer)'); sys.exit()
u=float(u); flag='  !! within \$50 of the ceiling' if u > $CEILING-50 else ''
print(f'spend: \${u:.2f} month to date (ceiling \${$CEILING}){flag}')" || echo "!! spend: unreadable (the answer is not JSON)"
else
  echo "!! spend: unreadable: $(echo "$BAL" | tail -1 | cut -c1-160)"
fi

# the queue: core's own summary and pause
core queue | python3 -c "
import json,sys
d=json.load(sys.stdin)
if d.get('paused'): print('!! queue PAUSED:', d.get('pause_reason'))
print('claimable:', d.get('claimable'), ' live leases:', d.get('live_leases'))
for j in d['jobs']:
    if j['state'] in ('QUEUED','LEASED','FAILED'):
        print(('!! ' if j['state']=='FAILED' else '   ') + f\"{j['kind']:13} {j['state']:7} {j['jobs']:4} jobs {j['first']}..{j['last']} build {str(j.get('build'))[:16]}\")" 2>/dev/null || echo "!! queue: unreadable (core not reachable)"

# jobs of the admitted build(s), done today vs pods that finished
sql "SELECT kind, state, count(*) FROM core.job j JOIN core.engine_build b USING (engine_build)
     WHERE b.withdrawn_at IS NULL AND (j.finished_at > now() - interval '24 hours' OR j.state IN ('QUEUED','LEASED','FAILED'))
     GROUP BY 1,2 ORDER BY 1,2" | sed 's/^/   jobs (24 h, running builds): /'

# pods: by state; errors with their last line; "nothing to claim" exits while work is claimable
if ! kubectl -n $NS get pods -l app=vyreon-worker --no-headers >/dev/null 2>&1; then
  echo "!! pods: unreadable (is the kubeconfig current?): the pod and node lines below are not true"
fi
kubectl -n $NS get pods -l app=vyreon-worker --no-headers 2>/dev/null | awk '{print $3}' | sort | uniq -c | sed 's/^/   pods: /'
# C-068 (audit opsurface ops.md 5): pods that fail before they claim charge no attempt; KEDA starts
# another every poll. Failed Jobs per ScaledJob in the last hour, and pods stuck before running
# (a Secret key missing: CreateContainerConfigError; no node: Pending) for over 10 minutes
kubectl -n $NS get jobs -l scaledjob.keda.sh/name -o jsonpath='{range .items[?(@.status.failed)]}{.metadata.labels.scaledjob\.keda\.sh/name}{" "}{.status.startTime}{"\n"}{end}' 2>/dev/null \
  | awk -v since="$(date -u -d '1 hour ago' '+%Y-%m-%dT%H:%M:%SZ')" '$2 >= since {n[$1]++} END {for (k in n) print "!! " n[k] " failed " k " Jobs in the last hour (a crash loop? see their pods: kubectl -n vyreon get pods -l app=vyreon-worker)"}'
kubectl -n $NS get pods -l app=vyreon-worker --no-headers 2>/dev/null \
  | awk '($3=="Pending"||$3=="ContainerCreating"||$3=="CreateContainerConfigError"||$3=="ImagePullBackOff"||$3=="ErrImagePull") && $5 ~ /(1[0-9]|[2-9][0-9])m|h|d/ {print "!! " $1 " " $3 " for " $5 " (`kubectl -n vyreon describe pod " $1 "`: events say why)"}'
for p in $(kubectl -n $NS get pods -l app=vyreon-worker --no-headers 2>/dev/null | awk '$3=="Error"||$3=="OOMKilled"||$3=="CrashLoopBackOff"{print $1}'); do
  echo "!! $p: $(kubectl -n $NS logs "$p" --tail=1 2>&1 | cut -c1-160)"
done
EMPTY=0; for p in $(kubectl -n $NS get pods -l app=vyreon-worker --no-headers 2>/dev/null | awk '$3=="Completed"{print $1}'); do
  kubectl -n $NS logs "$p" --tail=1 2>/dev/null | grep -q "to claim" && EMPTY=$((EMPTY+1))
done
[ "$EMPTY" -gt 0 ] && echo "   pods that found nothing to claim: $EMPTY (with work claimable this is the claim race, MILESTONES loose ends)"

# memory of day workers, from their own log (D-407 "memory after <T>")
for p in $(kubectl -n $NS get pods -l work=day-block --no-headers 2>/dev/null | awk '$3=="Running"{print $1}'); do
  echo "   $p memory: $(kubectl -n $NS logs "$p" 2>/dev/null | grep -o 'memory after [0-9-]*[^,;]*' | tail -3 | tr '\n' ' ')"
done

# nodes: worker nodes with no worker pod are idle and billing
BUSY=$(kubectl -n $NS get pods -l app=vyreon-worker -o wide --no-headers 2>/dev/null | grep -E "Running|Pending|ContainerCreating" | awk '{print $7}' | sort -u)
# C-068 round 2 (round2-core.md 19): a node list that cannot be read is said, never "0 nodes"
if NODES=$(kubectl get nodes --no-headers 2>&1); then
  for n in $(echo "$NODES" | awk '$1 ~ /^day/ {print $1}'); do
    echo "$BUSY" | grep -qx "$n" || echo "!! idle node $n (no worker pod): delete-idle.sh"
  done
  echo "   nodes: $(echo "$NODES" | awk 'NF && $1 ~ /^day/' | wc -l) worker, $(echo "$NODES" | awk 'NF && $1 !~ /^day/' | wc -l) other"
else
  echo "!! nodes: unreadable ($(echo "$NODES" | tail -1 | cut -c1-120)): how many run, and bill, is not known here; DigitalOcean's panel says"
fi

# stall: claimable work and no lease for a while
sql "SELECT CASE WHEN EXISTS (SELECT FROM core.claimable_job) AND NOT EXISTS (SELECT FROM core.lease WHERE outcome IS NULL)
     THEN '!! claimable work and no live lease: no worker is running' END" | grep . || true

# core's open problems (C-050)
core problems | head -12 | sed 's/^/   /'
