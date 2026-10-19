# Bluetooth connected-device monitoring

The Terminal Network overview includes a Bluetooth count and battery-coverage summary.
Press `b` for the connected-device report; arrow keys, Page Up/Down, Home and End scroll
it, and `b`/Esc returns to the existing view. This transient overlay does not change
released panel settings. The Web Bluetooth card has its own Details/Less switch and
participates in Expand all/Collapse all.

The source is the read-only Apple System Information Bluetooth report, acquired with
`system_profiler SPBluetoothDataType -json -detailLevel basic -timeout 5`. macmon uses
only `device_connected`, not paired-device lists, previously seen devices or stale
IORegistry serial-port objects. No device discovery, connection, pairing, radio
control or hardware writes occur. No additional privileges or dependencies are needed.

One worker belongs to the shared collector, with a 30-second cache, four-second hard
deadline and 1 MiB output bound. Slow/failed reports do not hold the metric loop's
cache lock. Normal collector exit, idle expiry and acquisition errors all drop the
worker supervisor, cancel/reap its private process group and join its thread.
The parent deadline fires before the reporter's own timeout can return partial data.

`/api/sample` adds an optional `bluetooth` object: collection `state`, controller
on/off or unknown, `sampled_at`, `age_ms`, and connected `devices` with session-only
opaque `id`, `name`, `kind` and reported `batteries`. Component labels are `main`,
`left`, `right` and `case`; only macOS-supplied integer percentage strings in 0–100
are accepted. A genuine 0% remains zero. An absent component is unavailable and is
never substituted with an average, a copied component or zero.

Before the first report, state is `warming`; missing or malformed controller/report
data is `unavailable`. A failed acquisition retains the previous report as `stale`.
Successful cached reports also become stale after 90 seconds. Report age includes
time retained by the collector, API and client. It measures when macmon read macOS's
cached report, not when a peripheral measured or transmitted its battery charge.
An empty successful report means no connected devices were reported at that time;
an unavailable report does not establish zero connected devices.

Device names and opaque identities are exposed only to the existing local loopback
peer/Host authority boundary used for process telemetry. Remote requests receive
`bluetooth: null`. Device addresses are used only as input to a randomly keyed
collector-session identity; addresses, serials, firmware and disconnected-device
records are discarded before storage/export. Legacy JSON, Prometheus, public library
types and released configuration fields are unchanged.

## Source evidence and tested coverage

[Apple System Information](https://support.apple.com/guide/system-information/system-information-syspr35536/mac)
documents its hardware report; the installed Apple `system_profiler(8)` manual
documents JSON, detail levels and bounded acquisition (its own timeout can yield a
partial report). The macOS 15.6.1 Apple `SPBluetoothReporter` binary contains the
four exact `device_batteryLevelMain/Left/Right/Case` keys. The public
[IOBluetoothDevice API](https://developer.apple.com/documentation/iobluetooth/iobluetoothdevice)
documents connected/paired state but supplies no general battery-percentage accessor
in the installed SDK. Cached System Information supplies a common source for both
connection and reported component charge without an active Bluetooth query.

Rootless source validation on an Apple M2 Max / Mac14,5 running macOS 15.6.1 (24G90)
returned in approximately 65 ms. Two connected peripherals were observed: a keyboard
without a reported battery, and a M585/M590 mouse with a 100% main battery. This
establishes partial coverage, not universal Bluetooth battery support. Separate
earbud/case handling, controller-off, disconnect/reconnect and unsupported data are
covered by anonymized parser/UI fixtures; actual AirPods component readings and
other macOS/hardware versions have not been verified.
