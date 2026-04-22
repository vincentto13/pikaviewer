# Third-Party Notices

PikaViewer itself is licensed under the MIT License (see `LICENSE`).

This document lists third-party components whose licenses require attribution
or specific notices when PikaViewer is distributed in binary form (e.g. the
macOS `.app`/`.dmg`, the Linux `.deb`, or the `.AppImage`). All Rust crate
dependencies are permissively licensed (MIT / Apache-2.0 / BSD / ISC / Zlib);
those notices are preserved by cargo at build time and the full dependency
list with licenses can be regenerated with:

```
cargo tree --format '{p} {l}'
```

The entries below cover the non-Rust / copyleft components that pull in
additional obligations.

---

## LibRaw (RAW decoding — when built with `--features iv-app/raw`)

- Upstream: <https://www.libraw.org/> — <https://github.com/LibRaw/LibRaw>
- License: dual-licensed **LGPL-2.1** OR **CDDL-1.0** (at the recipient's
  choice). PikaViewer **elects CDDL-1.0**.
- Linkage: C++ sources vendored by the `rsraw-sys` crate and **statically
  linked** into the PikaViewer binary via the `cc` crate. No modifications
  are made to LibRaw's source files.
- Source availability: the complete, corresponding LibRaw source used to
  build the distributed binary is available at
  <https://github.com/LibRaw/LibRaw> and in the build-time dependency tree
  under `rsraw-sys`. PikaViewer is built from the public repository
  <https://github.com/vincentto13/pikaviewer>.
- Copyright: Copyright (C) 2008-2024 LibRaw LLC. Portions derived from
  dcraw.c by David Coffin.

The CDDL-1.0 license text is included in the LibRaw upstream distribution
(`LICENSE.CDDL`) and reproduced at
<https://opensource.org/license/cddl-1-0>.

---

## libheif (HEIC/HEIF/AVIF decoding — when built with `--features iv-app/heic`)

- Upstream: <https://github.com/strukturag/libheif>
- License: **LGPL-3.0-or-later**
- Linkage: **dynamically linked**.
  - macOS: the required Homebrew dylibs (`libheif`, `libde265`, `libaom`,
    `libx265`, `libvmaf`, `libsharpyuv`) are bundled into
    `PikaViewer.app/Contents/Frameworks/` so the `.app` is self-contained.
    Users may replace these dylibs with compatible builds to satisfy the
    LGPL's relinking requirement.
  - Linux (`.deb`): libheif is pulled in as a package recommendation
    (`libheif1`) and linked against the system copy.
  - Linux (`.AppImage`): libheif and its transitive dependencies are bundled
    under `usr/lib/` and loaded via an `$ORIGIN/../lib` rpath. Users may
    replace these bundled libraries to satisfy the LGPL's relinking
    requirement.
- Copyright: Copyright (c) 2017-2024 Dirk Farin, struktur AG.

libheif's transitive dynamic dependencies that are also redistributed in the
macOS bundle / AppImage:

| Library     | License                       |
|-------------|-------------------------------|
| libde265    | LGPL-3.0-or-later             |
| x265        | GPL-2.0-or-later (shared lib) |
| libaom      | BSD-2-Clause-Patent           |
| libvmaf     | BSD-2-Clause                  |
| libsharpyuv | BSD-3-Clause (from libwebp)   |

The x265 shared library is distributed under the GPL-2.0-or-later. Because
x265 is dynamically linked (not statically linked into the PikaViewer
binary) and is invoked only as a separate shared object for its decoding
functionality, its license does not propagate to PikaViewer's own sources.
Users who wish to redistribute the combined work must comply with the GPL
for the x265 component, which is satisfied by providing the corresponding
source at <https://bitbucket.org/multicoreware/x265_git>.

---

## C++ runtime

Both LibRaw and libheif are C++ and therefore link against the platform C++
runtime (`libstdc++` on Linux, `libc++` on macOS). On Linux, the AppImage
intentionally does **not** bundle `libstdc++.so*` — it relies on the host
system's copy — so the binary runs against whichever `libstdc++` the host
provides. On macOS, `libc++` is part of the OS and is used as provided by
Apple.

---

## Offering of corresponding source

For the copyleft components above (LibRaw, libheif and its LGPL/GPL
transitive deps), the complete corresponding source code used to build the
distributed PikaViewer binaries is available from the upstream projects
linked in each section. No modifications are made to these upstream sources
by PikaViewer. On request, the PikaViewer maintainers will point to the
exact upstream tag or commit used for a given release; open an issue at
<https://github.com/vincentto13/pikaviewer/issues>.
