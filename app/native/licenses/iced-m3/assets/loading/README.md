# Canonical loading shapes

`morphs.bin` contains the seven **matched cubic pairs** used by the Material
loading indicator, exported from AndroidX's actual `Morph` implementation:
soft burst → cookie 9 → pentagon → pill → sunny → cookie 4 → oval → soft burst.
The runtime interpolates the corresponding control points. It does not resample
the shapes into polygons or approximate them with trigonometric radial functions.

Inputs are pinned to Material Components for Android revision
`d12048664f383e88148afb18e971aa6dd24ed42e`:

- [MaterialShapes.java](https://github.com/material-components/material-components-android/blob/d12048664f383e88148afb18e971aa6dd24ed42e/lib/java/com/google/android/material/shape/MaterialShapes.java)
- [LoadingIndicatorDrawingDelegate.java](https://github.com/material-components/material-components-android/blob/d12048664f383e88148afb18e971aa6dd24ed42e/lib/java/com/google/android/material/loadingindicator/LoadingIndicatorDrawingDelegate.java)
- [Dependency versions](https://github.com/material-components/material-components-android/blob/d12048664f383e88148afb18e971aa6dd24ed42e/gradle/libs.versions.toml): graphics-shapes **1.0.1**.

The data and reference SVGs are Apache-2.0 licensed. Keep `NOTICE` and
`LICENSE-APACHE-2.0.txt` when redistributing. Application acknowledgments can use
`iced_m3::loading::{LICENSE, NOTICE}`. The library's own code remains MIT.

## Regeneration

Run `python3 tools/generate_loading_shapes.py` from the repository after placing
these development inputs under `target/material-progress-source`:

1. The pinned `MaterialShapes.java` at its upstream relative path and the upstream
   `LICENSE` at that directory's root.
2. Google Maven artifact `androidx.graphics:graphics-shapes-android:1.0.1`:
   extract the AAR's `classes.jar` as `graphics-shapes.jar`.
3. `org.jetbrains.kotlin:kotlin-stdlib:2.0.21` as `kotlin-stdlib.jar` and
   `androidx.collection:collection-jvm:1.4.5` as `collection-jvm.jar`.
4. A temporary macOS Java 21 JDK with `*/Contents/Home/bin/{java,javac}` beneath
   that directory. The initial export used Amazon Corretto 21 for Apple Silicon.

`SHA256SUMS` records the exact source, jars and output used here. The generator
replaces only Android's affine matrix/point/rectangle types with small Java
adapters; polygon rounding, normalization and feature matching run in the
unmodified AndroidX library. No Java tooling or dependency is needed to build,
test or use the Rust crate after export.

The format is seven records, each a little-endian u32 cubic count followed by
that many pairs of `[f32; 8]` (start anchor, two controls, end anchor for each
endpoint shape). Coordinates are radially normalized to the unit circle.
The runtime uses a 34px shape inside a 48px container and the reference -90°
initial rotation. `reference/` contains independently exported original shape
outlines and `Morph.asCubics(0.5)` midpoints for the regression test.

Shape/morph raster comparisons allow a one-physical-pixel edge neighborhood at
6x shape scale, with at least 99.5% of channels identical. The SVG tessellator can
rasterize a subdivided cubic slightly differently from the unsplit original;
interior pixels must still match. Normal timed golden comparisons remain exact.
