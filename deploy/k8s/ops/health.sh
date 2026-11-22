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

echo "== $(date -u '+%Y-%m-%d %H:%M UTC') ($(TZ=America/Edmonton date '+%H:%M MT'))"

# money: DigitalOcean's own month-to-date usage
"$S/do.sh" GET /v2/customers/my/balance 2>/dev/null | python3 -c "
import json,sys
b=json.load(sys.stdin); u=float(b.get('month_to_date_usage') or 0)
flag='  !! within \$50 of the ceiling' if u > $CEILING-50 else ''
print(f'spend: \${u:.2f} month to date (ceiling \${$CEILING}){flag}')" || echo "!! spend: balance not readable"

# the queue: core's own summary and pause
core queue | python3 -c "
import json,sys
d=json.load(sys.stdin)
if d.get('paused'): print('!! queue PAUSED:', d.get('pause_reason'))
print('claimable:', d.get('claimable'), ' live leases:', d.get('live_leases'))
for j in d['jobs']:
    if j['state'] in ('QUEUED','LEASED','FAILED'):
        print(('!! ' if j['state']=='FAILED' else '   ') + f\"{j['kind']:13} {j['state']:7} {j['jobs']:4} jobs {j['first']}..{j['last']}\")"

# jobs of the admitted build(s), done today vs pods that finished
sql "SELECT kind, state, count(*) FROM core.job j JOIN core.engine_build b USING (engine_build)
     WHERE b.withdrawn_at IS NULL AND (j.finished_at > now() - interval '24 hours' OR j.state IN ('QUEUED','LEASED','FAILED'))
     GROUP BY 1,2 ORDER BY 1,2" | sed 's/^/   jobs (24 h, running builds): /'

# pods: by state; errors with their last line; "nothing to claim" exits while work is claimable
kubectl -n $NS get pods -l app=vyreon-worker --no-headers 2>/dev/null | awk '{print $3}' | sort | uniq -c | sed 's/^/   pods: /'
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
for n in $(kubectl get nodes --no-headers 2>/dev/null | awk '$1 ~ /^day/ {print $1}'); do
  echo "$BUSY" | grep -qx "$n" || echo "!! idle node $n (no worker pod): delete-idle.sh"
done
echo "   nodes: $(kubectl get nodes --no-headers 2>/dev/null | awk '$1 ~ /^day/' | wc -l) worker, $(kubectl get nodes --no-headers 2>/dev/null | awk '$1 !~ /^day/' | wc -l) other"

# stall: claimable work and no lease for a while
sql "SELECT CASE WHEN EXISTS (SELECT FROM core.claimable_job) AND NOT EXISTS (SELECT FROM core.lease WHERE outcome IS NULL)
     THEN '!! claimable work and no live lease: no worker is running' END" | grep . || true

# core's open problems (C-050)
core problems | head -12 | sed 's/^/   /'
