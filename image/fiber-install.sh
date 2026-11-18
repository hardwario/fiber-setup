#!/bin/bash
# FIBER Setup: installs the FIBER platform on the first start. Written onto the boot partition by
# FIBER Setup, beside the seed (fiber-seed.yaml) that holds the choice of components, their
# settings and the login for the services.
#
# cloud-init runs it once with --start (user-data runcmd): that puts fiber-install.service in
# place and starts it without waiting for it. The service does the work. It tries again 30 s
# after a failure and at every later start, until it has succeeded: no internet yet, an Ethernet
# cable plugged in only later, a power cut.
#
# The work, in four steps:
#   1. wait for the network, over plain HTTP: it answers before the clock is right;
#   2. move the clock forward to the date of that answer, if it is behind and NTP has not set it
#      (a fresh image starts with its build date, and TLS and apt need the right time);
#   3. download the installer of the FIBER platform release FIBER Setup names, and check its
#      sha256;
#   4. run it. It takes the seed, overwrites it and deletes it, and installs the components.
#
# While it runs it says what it does in an Avahi record (_fiber-setup._tcp), so that FIBER Setup
# can follow it without a login; the platform's own record (_fiber._tcp) says which component
# it installs. When the installation asks for a restart, the script restarts the FIBER, runs
# once more after it to say that it is done, and then removes its record.
#
# The log is /var/log/fiber-setup.log, copied to the boot partition as fiber-setup.log (readable
# over USB with the jumper on BOOT). By hand:
#   sudo bash /boot/firmware/fiber-install.sh              (restarts the FIBER when asked to)
#   sudo bash /boot/firmware/fiber-install.sh --no-reboot
set -uo pipefail

# Written in by FIBER Setup (catalog.rs): the release and the sha256 of its installer.
VERSION="@PLATFORM_VERSION@"
INSTALLER_SHA256="@INSTALLER_SHA256@"
# FIBER_INSTALLER_URL: another place for the same installer, for tests (its sha256 is checked
# all the same). The installer has FIBER_RELEASE_URL for its archive.
INSTALLER="${FIBER_INSTALLER_URL:-https://github.com/hardwario/fiber-platform/releases/download/v${VERSION}/install.sh}"
BOOT=/boot/firmware
LOG=/var/log/fiber-setup.log
STATE=/var/lib/fiber-setup
DONE=$STATE/done
UNIT=/etc/systemd/system/fiber-install.service
RECORD=/etc/avahi/services/fiber-setup.service
NET_WAIT=600 # seconds to wait for the network before the service tries again
# Plain HTTP, the FIBER's own package sources: any answer means the network is there, and it
# carries the date.
TIME_SOURCES=(http://deb.debian.org/debian/ http://archive.raspberrypi.com/debian/)

# --start, from cloud-init: the service, started without waiting for it. It is ordered after
# cloud-init's last stage, which runs this, so waiting for it would wait for ever.
start_service() {
    [ -f "$DONE" ] && return 0
    cat > "$UNIT" << EOF
[Unit]
Description=FIBER Setup: install the FIBER platform (until it has succeeded)
Wants=network-online.target
After=network-online.target cloud-final.service
ConditionPathExists=!${DONE}
StartLimitIntervalSec=0

[Service]
Type=oneshot
ExecStart=/bin/bash ${BOOT}/fiber-install.sh
Restart=on-failure
RestartSec=30s

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    systemctl enable --now --no-block fiber-install.service
}

# The Avahi record: what the script does, and what failed the last time, if anything. Avahi
# reads the directory again when a file is renamed into it (a name that does not end in .service
# is not read, so the file is written beside it first), but reads a file again only when its
# modification time has changed in whole seconds: the new file gets a second after the old one's
# when now is not later, or a second state within a second would never be seen.
publish() {
    [ -d "${RECORD%/*}" ] || return 0
    local error=""
    [ -f "$STATE/error" ] && error="    <txt-record>error=$(cat "$STATE/error")</txt-record>
"
    cat > "$RECORD.new" << EOF
<?xml version="1.0" standalone="no"?>
<!-- Written by FIBER Setup's first-start script while it installs; removed when it is done. -->
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">FIBER Setup %h</name>
  <service>
    <type>_fiber-setup._tcp</type>
    <port>22</port>
    <txt-record>state=$1</txt-record>
    <txt-record>release=${VERSION}</txt-record>
${error}  </service>
</service-group>
EOF
    local time old
    time=$(date +%s)
    old=$(stat -c %Y "$RECORD" 2> /dev/null || echo 0)
    [ "$time" -gt "$old" ] || time=$((old + 1))
    touch -d "@$time" "$RECORD.new"
    mv -f "$RECORD.new" "$RECORD"
}

copy_log() {
    sleep 1 # let tee write the last lines before the copy
    cp "$LOG" "${BOOT}/fiber-setup.log" 2> /dev/null || true
}

# A step failed: say which (FIBER Setup shows it) and why. The service tries again in 30 s.
stop() {
    printf '%s' "$1" > "$STATE/error"
    publish failed
    echo "STOP: $2"
    echo "=== fiber-install result: FAILED"
    copy_log
    exit 1
}

finished() {
    printf 'setup=fiber-install\nrelease=%s\nfinished=%s\n' "$VERSION" "$(date -Is)" > "$DONE"
    rm -f "$STATE/restart" "$STATE/error" "$STATE/install.sh" "$RECORD"
    systemctl disable fiber-install.service 2> /dev/null || true
    echo "=== fiber-install result: OK"
    copy_log
    exit 0
}

# Any answer over plain HTTP: prints its date.
web_date() {
    local url date
    for url in "${TIME_SOURCES[@]}"; do
        date=$(curl -sSI --max-time 10 "$url" 2> /dev/null | sed -n 's/^[Dd]ate: //p' | tr -d '\r')
        if [ -n "$date" ]; then
            printf '%s' "$date"
            return 0
        fi
    done
    return 1
}

wait_network() {
    echo "=== Step 1 of 4: waiting for the network (up to $((NET_WAIT / 60)) minutes)"
    publish network
    local deadline=$((SECONDS + NET_WAIT))
    until WEB_DATE=$(web_date); do
        [ "$SECONDS" -lt "$deadline" ] || stop network "no network: connect the Ethernet cable or check the Wi-Fi. It tries again by itself"
        sleep 5
    done
    echo "The network answers."
}

# Only ever forward: an answer over plain HTTP could come from anyone, and a clock that went back
# would make files look as if they came from the future.
set_clock() {
    echo "=== Step 2 of 4: the clock"
    if [ "$(timedatectl show -p NTPSynchronized --value 2> /dev/null)" = yes ]; then
        echo "Set by NTP: $(date -Is)"
        return
    fi
    local web now
    web=$(date -d "$WEB_DATE" +%s 2> /dev/null || echo 0)
    now=$(date +%s)
    if [ "$web" -gt $((now + 60)) ]; then
        date -s "@$web" > /dev/null && echo "Moved forward to $(date -Is), from the date of a plain HTTP answer (no NTP yet)"
    else
        echo "Not behind: $(date -Is)"
    fi
}

download() {
    echo "=== Step 3 of 4: downloading the installer of the FIBER platform ${VERSION}"
    publish download
    echo "From ${INSTALLER}"
    curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 -o "$STATE/install.sh.part" "$INSTALLER" ||
        stop download "could not download ${INSTALLER}. It tries again by itself"
    local got
    got=$(sha256sum "$STATE/install.sh.part" | cut -d ' ' -f 1)
    if [ "$got" != "$INSTALLER_SHA256" ]; then
        rm -f "$STATE/install.sh.part"
        stop checksum "the installer is not the one FIBER Setup names: its sha256 is $got, not ${INSTALLER_SHA256}"
    fi
    mv -f "$STATE/install.sh.part" "$STATE/install.sh"
    echo "Its sha256 is right: $got"
}

install() {
    echo "=== Step 4 of 4: installing the FIBER platform ${VERSION}"
    publish install
    FIBER_NO_REBOOT=1 bash "$STATE/install.sh"
    local code=$?
    case "$code" in
        0) finished ;;
        # 10: done, and a restart is needed (the board's config.txt changed). 20: partly done;
        # the platform tries again by itself, also after the restart, which the board's
        # settings may need all the same.
        10 | 20)
            cat /proc/sys/kernel/random/boot_id > "$STATE/restart"
            publish restart
            echo "=== The installation asks for a restart (installer exit $code)"
            copy_log
            if [ "$REBOOT" -eq 1 ]; then
                systemctl reboot --no-block
            else
                echo "--no-reboot: restart the FIBER yourself, then run this script once more."
            fi
            exit 0
            ;;
        *) stop installer "the installer failed (exit $code); the lines above say why. It tries again by itself" ;;
    esac
}

main() {
    if [ "${1:-}" = --start ]; then
        start_service
        exit 0
    fi
    REBOOT=1
    [ "${1:-}" = --no-reboot ] && REBOOT=0

    # One run at a time: the service and a person could overlap.
    exec 9> /run/fiber-install.lock
    flock -n 9 || {
        echo "fiber-install: already running"
        exit 0
    }
    exec > >(tee -a "$LOG") 2>&1

    echo "=== fiber-install, $(date -Is)"
    if [ -f "$DONE" ]; then
        echo "Installed already ($DONE)."
        exit 0
    fi
    mkdir -p "$STATE"
    # The installation asked for a restart, and this is the start after it.
    if [ -f "$STATE/restart" ] && [ "$(cat "$STATE/restart")" != "$(cat /proc/sys/kernel/random/boot_id)" ]; then
        echo "Restarted after the installation."
        finished
    fi
    # Wi-Fi may be set later from the phone app; the radio must not stay blocked.
    rfkill unblock wifi 2> /dev/null || true

    wait_network
    set_clock
    download
    install
}

main "$@"
