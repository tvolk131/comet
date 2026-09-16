# Roboto

`Roboto-Variable.ttf` is the unmodified upright Roboto variable font, version
3.015, distributed by [Google Fonts](https://github.com/google/fonts/tree/main/ofl/roboto).
It includes the weight axis used for Regular (400) and Medium (500).

- Downloaded 2026-09-12 from [the font file](https://raw.githubusercontent.com/google/fonts/main/ofl/roboto/Roboto%5Bwdth,wght%5D.ttf).
- SHA-256: `d7598e12c5dbef095ff8272cfc55da0250bd07fbdecbac8a530b9b277872a134`.
- Upstream provenance is preserved in [METADATA.pb](METADATA.pb):
  [googlefonts/roboto-classic](https://github.com/googlefonts/roboto-classic),
  commit `91d5d3e5b81efa04a77925cc609fdcdd7ee663d1`.
- The font is licensed under the **SIL Open Font License 1.1**, reproduced in
  [OFL.txt](OFL.txt). The Rust library's MIT license does not replace the font license.

The library embeds the font and its license and registers the font once with
iced's shared font database. Applications redistributing the font must retain
its copyright and license notice. `iced_m3::fonts::LICENSE` also exposes
the notice for application acknowledgments.
