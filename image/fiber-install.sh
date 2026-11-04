#!/bin/bash
# FIBER Setup: installs the FIBER software on the first start. Written onto the boot partition by
# FIBER Setup; cloud-init runs it once (user-data runcmd), and fiber-install.service runs it again
# at every later start until it has succeeded (no internet on the first start, a power cut).
#
# It waits for the internet (first-boot Wi-Fi joins late) and for the clock (a fresh image starts
# with its build date, which breaks TLS and apt), then runs the public one-command installer:
#
#   curl -fsSL https://raw.githubusercontent.com/hardwario/fiber-agent/main/bootstrap.sh | sudo bash
#
# with FIBER_NO_REBOOT=1, so this script writes its result first and then restarts the FIBER itself
# (Raspberry Pi OS's cloud-init has no power_state module).
#
# Result: exit code 0 = installed, then a restart. The log is /var/log/fiber-setup.log, copied to the
# boot partition as fiber-setup.log (readable over USB with the jumper on BOOT). By hand:
#   sudo bash /boot/firmware/fiber-install.sh            (restarts at the end)
#   sudo bash /boot/firmware/fiber-install.sh --no-reboot
set -uo pipefail

INSTALLER=https://raw.githubusercontent.com/hardwario/fiber-agent/main/bootstrap.sh
BOOT=/boot/firmware
LOG=/var/log/fiber-setup.log
DONE=/var/lib/fiber-setup/done
UNIT=/etc/systemd/system/fiber-install.service
NET_WAIT=600    # seconds to wait for the internet
CLOCK_WAIT=120  # seconds to wait for the clock (NTP)

REBOOT=1
[ "${1:-}" = "--no-reboot" ] && REBOOT=0

# One run at a time (cloud-init, the retry service and a person could overlap).
exec 9> /run/fiber-install.lock
flock -n 9 || { echo "fiber-install: already running"; exit 0; }

exec > >(tee -a "$LOG") 2>&1

finish() {
    local rc=$1
    if [ "$rc" -eq 0 ]; then
        echo "=== fiber-install result: OK"
    else
        echo "=== fiber-install result: FAILED (exit $rc)"
    fi
    sleep 1   # let tee write the last lines before the copy
    cp "$LOG" "${BOOT}/fiber-setup.log" 2>/dev/null || true
    if [ "$rc" -eq 0 ] && [ "$REBOOT" -eq 1 ]; then
        systemctl reboot --no-block
    fi
    exit "$rc"
}

stop() {
    echo "STOP: $*"
    finish 1
}

echo "=== fiber-install, $(date -Is)"
[ -f "$DONE" ] && { echo "already installed ($DONE)"; exit 0; }

# Try again at every start until it has worked; the condition stops it once DONE exists.
if [ ! -f "$UNIT" ]; then
    cat > "$UNIT" <<EOF
[Unit]
Description=FIBER Setup: install the FIBER software (until it has succeeded)
Wants=network-online.target
After=network-online.target cloud-final.service
ConditionPathExists=!${DONE}

[Service]
Type=oneshot
ExecStart=/bin/bash ${BOOT}/fiber-install.sh

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    systemctl enable fiber-install.service
fi

# cloud-init leaves the image's 127.0.1.1 line (manage_etc_hosts is off, so it does not fight the
# FIBER software over /etc/hosts later); give this start's name to it once, so sudo resolves it.
if [ -f /etc/hosts ] && [ ! -L /etc/hosts ]; then
    sed -i "s/^127\.0\.1\.1[[:space:]].*/127.0.1.1\t$(hostname)/" /etc/hosts
fi
# The FIBER software sets the Wi-Fi from the phone app later; the radio must not stay blocked.
rfkill unblock wifi 2>/dev/null || true

echo "=== Step 1 of 3: waiting for the internet (up to $((NET_WAIT / 60)) min)"
deadline=$((SECONDS + NET_WAIT))
until curl -fsS --max-time 10 -o /dev/null https://github.com; do
    [ "$SECONDS" -lt "$deadline" ] || stop "no internet: connect the Ethernet cable or check the Wi-Fi. It tries again at the next start, or now: sudo bash ${BOOT}/fiber-install.sh"
    sleep 5
done

echo "=== Step 2 of 3: waiting for the clock"
deadline=$((SECONDS + CLOCK_WAIT))
until [ "$(timedatectl show -p NTPSynchronized --value 2>/dev/null)" = yes ]; do
    if [ "$SECONDS" -ge "$deadline" ]; then
        # NTP blocked here (UDP 123): take the time from the web server's Date header instead, or
        # apt refuses its package lists as "not valid yet".
        web=$(curl -fsSI --max-time 10 https://github.com | sed -n 's/^[Dd]ate: //p' | tr -d '\r')
        [ -n "$web" ] && date -s "$web" > /dev/null && echo "clock set from github.com (no NTP)"
        break
    fi
    sleep 2
done
echo "clock: $(date -Is)"

echo "=== Step 3 of 3: curl -fsSL ${INSTALLER} | sudo bash"
curl -fsSL "$INSTALLER" | sudo FIBER_NO_REBOOT=1 bash
codes=("${PIPESTATUS[@]}")
[ "${codes[0]}" -eq 0 ] || stop "could not download the installer (curl exit ${codes[0]}); it tries again at the next start"
[ "${codes[1]}" -eq 0 ] || stop "the installer failed (exit ${codes[1]}); the lines above say why. It tries again at the next start, or now: sudo bash ${BOOT}/fiber-install.sh"

mkdir -p "${DONE%/*}"
printf 'setup=fiber-install\nversion=%s\nfinished=%s\n' "$(fiber version 2>/dev/null || echo unknown)" "$(date -Is)" > "$DONE"
systemctl disable fiber-install.service 2>/dev/null || true
finish 0
