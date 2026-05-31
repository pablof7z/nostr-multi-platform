---
title: CSS iPhone Device Mockup
slug: css-iphone-device-mockup
summary: The device mockup frame has a fixed width of approximately 260px, a background color of #141414, and a border-radius of 44px
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:1231660f-79c1-4b38-9651-9111cc20afb0
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# CSS iPhone Device Mockup

## Device Frame

The device mockup frame has a fixed width of approximately 260px, a background color of #141414, and a border-radius of 44px. Volume and power buttons are rendered on the bezel via CSS ::before and ::after pseudo-elements. A home indicator bar is displayed at the bottom of the device frame. [^12316-1]


## Screen Area

Screenshot images are displayed inside a pure-CSS iPhone device bezel mockup with a fixed 9:19.5 screen aspect ratio. The CSS device mockup frame is kept for screenshots (not removed), with object-fit: contain and background #f2f2f7 so full screenshots display correctly without zooming or cropping. A Dynamic Island pill element is positioned at the top of the screen area. [^12316-2]

<!-- citations: [^12316-2] [^53838-3] -->
## Fallback Placeholder

When a screenshot image fails to load, a placeholder tile is shown inside the device mockup screen area with a message to build and run NmpGallery to generate the screenshot. [^12316-3]

## Layout

The screenshots container uses a flex layout with flex-wrap for horizontal flow of multiple device mockups. [^12316-4]
## See Also

