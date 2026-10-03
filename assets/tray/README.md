These 32 px PNGs are raster copies of the corresponding SVGs in `apps/ui/public/icons/`, except `window.png`, whose source is `window.svg` here. Native tray menus cannot display the SVGs directly on every platform. The artwork sits inside a transparent 32 px canvas so macOS can display it at 16 points without making it look oversized. The raster color is neutral gray; macOS treats the images as templates and tints them for the menu appearance.

To regenerate an icon after its UI SVG changes:

```sh
rsvg-convert -w 24 -h 24 apps/ui/public/icons/ICON.svg | magick png:- -fill '#777777' -colorize 100 -background none -gravity center -extent 32x32 assets/tray/ICON.png
```

`triangle.png` uses 28 × 28 px before centering because its SVG has wider internal margins. `window.png` is generated from `window.svg` with the regular 24 × 24 px size.

## macOS menu bar icon

`macos/status.svg` is the editable monochrome source for the macOS menu bar icon. `macos/status.png` (22 × 22 pixels) and `macos/status@2x.png` (44 × 44 pixels) are the 1x and Retina images embedded by `crates/platform/src/integration/macos.rs`. The host combines them into one 22-point template image, and AppKit tints it for the menu bar. This is separate from the full-color application icon in `assets/icon.svg`.

Keep the SVG and both PNGs together when changing the artwork. The SVG retains the rewind arrow and winking skull with transparent negative space; the PNGs are the runtime assets.
