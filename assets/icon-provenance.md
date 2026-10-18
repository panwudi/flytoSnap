# Application icon

The user approved the B2 ivory viewfinder direction on 2026-10-18. Its mist-blue panel and cream relief clouds share the enamel housing's physical material style.

- Initial B and B2 reference edit: flyto_video, Seedream `doubao-seedream-5-0-260128`.
- Production alpha extraction: Codex, built-in imagegen, 2026-10-18.
- `app-icon-source.png`: transparent source artwork.
- `app-icon.png`: production 1024px artwork, exported by `flytosnap --export-icon assets/app-icon.png`.
- `src/macos/assets.rs`: native export and silhouette clipping. The source raster is a build input, not embedded in the executable.
- The menu bar and capture mode menu use the same color viewfinder artwork.

## Extraction prompt

Use case: background-extraction. Asset type: production macOS application icon, transparent PNG. Edit this approved icon only to remove the entire neutral-gray studio background and floor and their cast ground shadow, leaving actual transparent alpha outside the physical object. Preserve the exact ivory enamel body, thin silver rim, slate inner frame, mist-blue panel, two cream bas-relief clouds, orange button, perspective, colors, highlights, proportions, and all object details. Do not redesign, redraw, add a tile, add text, or change the sky. Center and uniformly scale the intact cutout so the object's longest dimension occupies about 90 percent of the square canvas with clear padding on all sides; keep the entire object visible. Output one clean square high-resolution transparent production icon.

## Alpha refinement prompt

Clean the alpha mask of this production app icon. Keep the entire physical ivory device and all its details unchanged. Remove all disconnected floating fragments, white flecks above the object, colored/yellow/red speckles near the lower edge, gray fringe outside the object, and any background or floor residue. Everything outside the single continuous physical object's silhouette must be fully transparent with zero alpha. Keep a smooth antialiased silhouette edge, no halo, no extra shadow. Do not redraw or change any part of the device. One square transparent PNG.
