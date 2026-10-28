# 0004: License — GPL-3.0-or-later

The project's code was MIT-licensed (charter, LICENSE, Cargo `license`). The
SPU2's 4-tap Gaussian resampling needs the hardware's 256 × 4 coefficient
table, which is not in the disc, the ELF, the IRX modules or the BIOS files;
the sources at hand were PCSX2 (GPL-3.0-or-later) and nocash's psx-spx, so the
interpolation stayed nearest-neighbour (unknowns.md "SPU Gaussian resampling",
BLOCKED: provenance).

Decision (approved by the project owner, 2026-10-27): the project is licensed
GPL-3.0-or-later, the terms of PCSX2's source files (SPDX `GPL-3.0+`), and
PCSX2 code may be reused with its copyright notices kept and recorded in
THIRD_PARTY.md. LICENSE holds the GPLv3 text (SHA-256 8ceb4b9e…5b903, copied
from the pinned PCSX2 checkout's COPYING.GPLv3); the workspace's Cargo
`license` is `GPL-3.0-or-later`.

Checked before relicensing: every commit is the project's own (440 by the
owner, two early ones under a local placeholder identity, 2026-10-12); the
consulted and vendored research tools are MIT, LGPL-2.1 or Apache-2.0 and
none ships with the runtime; the 438 external Cargo packages are under
MIT, Apache-2.0, Zlib, BSD, ISC, Unlicense, MIT-0 or CC0 terms (one offers
Apache-2.0 OR GPL-2.0-only and is taken under Apache-2.0), all compatible
with GPLv3; the fdlibm notice is permissive. The full transitive notice
audit remains part of M21.

Consequences: binaries built from this project are distributed under the
GPLv3's terms (corresponding source with them); retail game files and BIOS
data stay user-supplied and outside the license, as before. Earlier MIT
releases of the code, if any were made, keep their MIT grant.
