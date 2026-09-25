# macOS menu bar icon

`SaveScummerTemplate.svg` is the editable monochrome master. The two PNGs are black artwork with alpha transparency, designed for a 22 x 22 point image:

- `SaveScummerTemplate.png`: 22 x 22 pixels (1x).
- `SaveScummerTemplate@2x.png`: 44 x 44 pixels (Retina / 2x).
- `preview.png`: simulated light and dark tints, enlarged and at native pixel sizes. This is a design preview, not a macOS screenshot or a runtime asset.

The template retains the rewind arrow and winking skull. The skull is scaled to 90% around its center to increase clearance from the arrow. Both red eye shapes from the color logo are retained as solid monochrome details inside transparent eye sockets. The original dark badge, eyebrow details and tiny pupil highlight are omitted, and the tooth gaps are widened. Negative space is transparent, not white.

For a future AppKit port, bundle both PNGs as image representations of the same named image, or import them into an Xcode image set at 1x and 2x with **Render As: Template Image**. Set the logical image size to 22 points, not 44 points on Retina screens. For example, after the image has been included in the bundle:

```swift
if let image = NSImage(named: "SaveScummerTemplate") {
    image.isTemplate = true
    image.size = NSSize(width: 22, height: 22)
    statusItem.button?.image = image
    statusItem.button?.imagePosition = .imageOnly
    statusItem.button?.toolTip = "SaveScummer"
}
```

The application must retain its `NSStatusItem`. AppKit supplies the template's appearance; do not select separate black and white images yourself. The SVG is the editing source; the PNGs are the prepared runtime assets. These menu bar assets are separate from the full-color application/Dock icon.

The current platform integration supports Windows only. These assets are not wired into a macOS implementation and have not been tested in a live macOS menu bar. Adjust logical size and optical spacing during that integration if needed.

References:
- https://developer.apple.com/documentation/appkit/nsimage/istemplate
- https://developer.apple.com/documentation/appkit/nsimage
- https://developer.apple.com/documentation/appkit/nsstatusbarbutton
