# Reference IPC setup investigation

Installed reference: PCSX2 2.6.3+dfsg-1build1. The checkout supplied by the user
is a different revision, so this UI investigation also inspected official tag
v2.6.3 at commit bc8151d2a46d4aba039ea5580afbfc7bfcf6d730.

The user's disabled Slot field has a concrete source explanation: in the
[v2.6.3 setting binder](https://github.com/PCSX2/pcsx2/blob/v2.6.3/pcsx2-qt/SettingWidgetBinder.h),
the nullable integer QLineEdit accessor disables the widget when binding a
per-game override. The PINE slot is such a QLineEdit. The observed private
game-specific INI contained EnablePINE=true, while the global INI still had
EnablePINE=false and slot 28011. PINE must be configured in the global settings
for this reference workflow; the disabled slot does not prove a general ban
on editing settings while a game is loaded.

The [v2.6.3 VM manager](https://github.com/PCSX2/pcsx2/blob/v2.6.3/pcsx2/VMManager.cpp)
calls ReloadPINE during startup and disc changes. Its ordinary settings-apply
path has no ReloadPINE call. Use a saved-state-preserving restart after enabling
global PINE, rather than assuming a live checkbox change starts the server.
Hardcore mode also forces PINE off, but the established private configuration
has achievements and challenge mode disabled; it does not explain this case.

tools/python/launch_reference.py sets global EnablePINE and the selected slot
inside private/reference/config/PCSX2/, preserving backups and unrelated settings.
It removes only PINE overrides from the private game-specific INI. It refuses
to modify/relaunch the recognized reference while that process is running.
A supplied existing private state starts paused. The launcher verifies the
disc hash and records the configuration/state identities before launching.
Synthetic tests cover the global settings, backups and preservation of unrelated
preferences. The state-preserving live relaunch and frozen/live probes now pass.

The default slot 28011 is sufficient. On Linux its socket is pcsx2.sock;
other slots append .<slot>. The memory probe now reads the configured global
slot, so changing to 28012 is optional.

The user saved private/reference/captures/poison-fields.p2s. It is verified and
preserved as poison-fields-original.p2s. All automated GUI work now runs through
authenticated Xvfb at the user's request. Launch helpers require this session;
they cannot fall back to the desktop. Start the virtual display and then reload:

```sh
xvfb-run -a -n 191 -e private/reports/xvfb.log -s '-screen 0 1280x1024x24 -nolisten tcp' python3 tools/python/virtual_display.py
# In a second terminal, while that virtual session is running:
python3 tools/python/launch_reference.py --diagnostic-hotkeys --state private/reference/captures/poison-fields.p2s
python3 tools/python/pine_probe.py --out private/reports/poison-fields-pine.json
```

Xvfb requires Mesa software presentation (lavapipe/llvmpipe), selected only in
the child process environment. The physical AMD Vulkan presentation queue cannot
present to this virtual server. Reference configuration identities are recorded.
F10 advances a frame; F7 loads private slot 1; F11 saves private slot 2.
The bounded trace tool verifies one vblank per advance and releases held input.
See reference-baseline.md for actual results and remaining timing limitations.

The user is not expected to change their normal emulator configuration or saves.
All of this is isolated development-reference tooling; native gameplay will not
depend on PCSX2, PINE or a BIOS.

Virtual-display guard follow-up: the authenticated xvfb-run wrapper can exit while
its Xvfb server and Python holder remain alive under user systemd. The holder's
current parent is insufficient as a lifetime check. The session now records and
verifies actual Xvfb PID/start-time and exact display/auth arguments on every
GUI operation; it still refuses Xorg/Xwayland or an authentication/PID mismatch.
The existing :191 server was independently verified during this migration.
A synthetic process fixture covers these rejection cases. No desktop fallback.
