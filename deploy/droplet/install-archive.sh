#!/usr/bin/env bash
# Install the nightly archive of captured inputs on the engine droplet (run as root from a copy of
# this directory). Needs rclone (apt). The bucket and its keys are added by the owner; until then the
# archive is written to the volume only.
set -euo pipefail
D=$(cd "$(dirname "$0")" && pwd)
command -v rclone >/dev/null || apt-get install -y rclone
install -m 755 "$D/archive-perishables.sh" /opt/vyreon/bin/
[ -f /etc/vyreon-archive.env ] || install -m 600 "$D/archive.env.example" /etc/vyreon-archive.env
install -d -m 700 /etc/vyreon
install -m 644 "$D"/systemd/vyreon-archive.{service,timer} /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now vyreon-archive.timer
