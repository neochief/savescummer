These 32 px PNGs are raster copies of the corresponding SVGs in `apps/ui/public/icons/`. Native tray menus cannot display the SVGs directly on every platform. The raster color is neutral gray; macOS treats the images as templates and tints them for the menu appearance.

To regenerate an icon after its UI SVG changes:

```sh
rsvg-convert -w 32 -h 32 apps/ui/public/icons/ICON.svg | magick png:- -fill '#777777' -colorize 100 assets/tray/ICON.png
```
