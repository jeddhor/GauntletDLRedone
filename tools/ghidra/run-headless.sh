#!/usr/bin/env bash
# Use the pinned Snap contents with isolated preferences and the local PS2 module.
set -euo pipefail
gdl_root="$(cd "$(dirname "$0")/../.." && pwd)"
gdl_ghidra=/snap/ghidra/47/ghidra
gdl_module="$gdl_root/private/ghidra-extension/ghidra-emotionengine-reloaded"
if [[ ! -f "$gdl_module/data/languages/emotionengine.ldefs" ]]; then
  echo "Missing local PS2 extension; see docs/toolchain.md" >&2
  exit 1
fi
mkdir -p "$gdl_root/private/ghidra-config" "$gdl_root/private/ghidra-cache" "$gdl_root/private/ghidra-user"
XDG_CONFIG_HOME="$gdl_root/private/ghidra-config" XDG_CACHE_HOME="$gdl_root/private/ghidra-cache" \
/usr/lib/jvm/java-21-openjdk-amd64/bin/java -Xmx4G -XX:ParallelGCThreads=2 -XX:CICompilerCount=2 \
  -Djava.system.class.loader=ghidra.GhidraClassLoader -Dfile.encoding=UTF8 \
  -Duser.country=US -Duser.language=en -Djava.awt.headless=true \
  -Duser.home="$gdl_root/private/ghidra-user" -Dghidra.external.modules="$gdl_module" \
  -cp "$gdl_ghidra/Ghidra/Framework/Utility/lib/Utility.jar" \
  ghidra.Ghidra ghidra.app.util.headless.AnalyzeHeadless "$@"
