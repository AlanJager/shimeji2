# Attribution for the bundled shime pack

The images, the two behavior XML files, and the three licence texts in this directory are copied byte-for-byte from Shimeji for Mac (`/tmp/shimeji4mac`, the AlanJager shimeji4mac tree).

Copied:

- `img/shime1.png` through `img/shime46.png`
- `conf/動作.xml` (actions) and `conf/行動.xml` (behaviors)
- `LICENSE.txt` (English zlib/libpng notice, UTF-8)
- `ライセンス.txt` (Japanese translation of that notice, Shift_JIS)
- `読んでね.txt` (the upstream "please read" file, Shift_JIS; the name on disk is in Unicode NFD, as it was upstream)

Not copied: `img/Thumbs.db`, `img/FixPngImage.jar`, `img/icon.png`, the Java sources, and the JNA jars. `CHECKSUMS.sha256` covers the 46 PNGs and the two XML files only. The licence texts are present as themselves.

The Japanese files were not converted to UTF-8. The quotations below are those files decoded as Shift_JIS (CP932).

## What the files say about the art

None of the three files mentions the character drawings as a separate work. They license "this software", say the program follows zlib/libpng, note that JNA is LGPL, and allow magazine and website introductions. The pictures ship inside that software, and the texts never say whether the mascot art is included in the grant or held back.

### `LICENSE.txt`

Copyright line:

> Copyright (c) 2009-2011 Yuki Yamada, 2011-2012 Yusaku Hashimoto, 2016 AlanJager

The grant, which is the whole of what this file says:

> This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held liable for any damages arising from the use of this software.
>
> Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:
>
> 1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.
>
> 2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.
>
> 3. This notice may not be removed or altered from any source distribution.

"This software" is the subject. Images, frames, and the shimeji character are not named.

### `ライセンス.txt`

`読んでね.txt` says this file is the Japanese translation of the zlib/libpng licence. The copyright line is the same English line as in `LICENSE.txt`. The body:

> 本ソフトウェアは「現状のまま」で、明示であるか暗黙であるかを問わず、何らの保証もなく提供されます。 本ソフトウェアの使用によって生じるいかなる損害についても、作者は一切の責任を負わないものとします。
>
> 以下の制限に従う限り、商用アプリケーションを含めて、本ソフトウェアを任意の目的に使用し、自由に改変して再頒布することをすべての人に許可します。
>
> 1.本ソフトウェアの出自について虚偽の表示をしてはなりません。あなたがオリジナルのソフトウェアを作成したと主張してはなりません。 あなたが本ソフトウェアを製品内で使用する場合、製品の文書に謝辞を入れていただければ幸いですが、必須ではありません。
> 2.ソースを変更した場合は、そのことを明示しなければなりません。オリジナルのソフトウェアであるという虚偽の表示をしてはなりません。
> 3.ソースの頒布物から、この表示を削除したり、表示の内容を変更したりしてはなりません。

Translation:

> This software is provided "as is", with no warranty of any kind, whether express or implied. The authors take no responsibility for any damage arising from the use of this software.
>
> Anyone may use this software for any purpose, including commercial applications, and may freely modify and redistribute it, subject to the following restrictions:
>
> 1. You must not misrepresent the origin of this software. You must not claim that you created the original software. If you use this software in a product, an acknowledgment in the product's documentation would be appreciated, but it is not required.
> 2. If you change the source, you must state that fact plainly. You must not misrepresent it as being the original software.
> 3. You must not remove this notice from a source distribution, or change the contents of the notice.

Again the subject is 本ソフトウェア ("this software"). The pictures are not mentioned.

### `読んでね.txt`

The file is mostly how to start, quit, and uninstall the Mac app, plus a changelog. Three sections bear on rights:

> ====ソースについて====
>
> プログラマな方はご自由にどうぞ。
>
> https://github.com/nonowarn/shimeji4mac
>
> ライセンスは zlib/libpng に従います。
> 日本語訳を ライセンス.txt に置きます。

Translation:

> About the source
>
> Programmers are welcome to do as they like.
>
> https://github.com/nonowarn/shimeji4mac
>
> The licence follows zlib/libpng.
> The Japanese translation is in ライセンス.txt.

> ====ライブラリについて====
>
> lib/jna.jar および lib/platform.jar は JNA のライブラリです。
> JNA は LGPL に従います。

Translation:

> About the libraries
>
> lib/jna.jar and lib/platform.jar are JNA libraries.
> JNA follows the LGPL.

shimeji2 does not ship those jars. The LGPL note does not apply to this repository.

> ====雑誌への掲載、サイトでの紹介等====
>
> 自由に行ってください。
> 事前の承諾は不要です。

Translation:

> Publication in magazines, introductions on websites, and the like
>
> Please feel free.
> Prior permission is not required.

That is permission to write about the program, not a separate grant for the character art.

The rest of the file (how to launch `Shimeji.app`, how to quit from the menu-bar icon, how to delete the unzipped folder, where the FAQs are, and the changelog through v2.2.0) does not discuss copyright. The v2.2.0 note mentions images only as a feature: if you provide `shime-r.png` next to `shime.png`, the flipped image is used. It does not license those files.

The question section points at the original project and the author contact:

> 公式サイトに FAQ があります。
> http://www.group-finity.com/Shimeji/
>
> しめじ for Mac に Mac 版特有の機能についての FAQ があります。
> http://nonowarn.jp/shimeji
>
> 作者にメール
> support@group-finity.com

Translation: the official site has an FAQ at the group-finity URL; Shimeji for Mac has an FAQ for Mac-only features; email the author at support@group-finity.com.

## Reading

The files apply zlib/libpng to the software and name Yuki Yamada (2009–2011), Yusaku Hashimoto (2011–2012), and AlanJager (2016). They never say the mascot drawings are in or out of that grant. Treating the PNGs as "part of this software" and treating them as a separate artwork are both consistent with the text. The art licence is ambiguous.

shimeji2 keeps this shime as the bundled default, keeps the original notice, and keeps this attribution. A replacement mascot, if one is needed, is a follow-up that depends on forge. It is not part of this scaffold.

Upstream:

- Shimeji, group-finity (Yuki Yamada), 2009–2011. <http://www.group-finity.com/Shimeji/>
- Shimeji for Mac, Yusaku Hashimoto, 2011–2012. <https://github.com/nonowarn/shimeji4mac>
- shimeji4mac, AlanJager, 2016. <https://github.com/AlanJager/shimeji4mac>
