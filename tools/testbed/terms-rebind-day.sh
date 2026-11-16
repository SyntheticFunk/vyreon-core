#!/usr/bin/env bash
# re-derive quote-file terms and markers under the deployed build (no provider call), rebind the ACT01 cohorts, then run the day
set -a; . /etc/vyreon.env; set +a
export VYREON_PG_FILE=/opt/vyreon-test/pg VYREON_KEEP_TRANSIENT=1 VYREON_WEBHOOK_FILE=/nonexistent VYREON_TAPE_HISTORY_FROM=2025-12-10 VYREON_DISK_RESERVE_PERCENT=10
unset VYREON_NOTIFY_STATE
cd /opt/vyreon-test
nice bin/vyreon-inputs terms --home "$VYREON_HOME" --bin /opt/vyreon-test/bin "$1" > "$2.terms.log" 2>&1
echo "terms exit $?" >> "$2.terms.log"
nice bin/act01_auto_source --rebind "$VYREON_HOME/h5-act01-auto/$1" > "$2.rebind.log" 2>&1
echo "rebind exit $?" >> "$2.rebind.log"
rm -f "$2.done"; nice ./run-day.sh "$1" "$2-${1//-/}" > "$2.log" 2>&1; echo $? > "$2.done"
