# Notice

shimeji2 is an altered version of Shimeji. It is a Rust rewrite, and it is not the original software. Do not represent it as the original Shimeji, Shimeji for Mac, or shimeji4mac.

## Original work

- Shimeji, by group-finity (Yuki Yamada), 2009–2011. <http://www.group-finity.com/Shimeji/>
- Shimeji for Mac, by Yusaku Hashimoto, 2011–2012. <https://github.com/nonowarn/shimeji4mac>
- shimeji4mac, by AlanJager, 2016. <https://github.com/AlanJager/shimeji4mac>

The bundled default character under `assets/packs/shime/` is copied byte-for-byte from shimeji4mac (`img/shime1.png` through `img/shime46.png`, `conf/動作.xml`, `conf/行動.xml`, and the upstream licence texts). What those texts say about the art is recorded in `assets/packs/shime/ATTRIBUTION.md`. The art licence is ambiguous; the character stays the bundled default, with this attribution.

JNA (`lib/jna.jar` and `lib/platform.jar`, LGPL) was part of the Java port. Those jars are not included in shimeji2.

## Original zlib/libpng notice (unaltered)

```
Copyright (c) 2009-2011 Yuki Yamada, 2011-2012 Yusaku Hashimoto, 2016 AlanJager

This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
```
