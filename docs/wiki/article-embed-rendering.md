---
title: Article Embed Rendering
slug: article-embed-rendering
summary: The article embed component is rendered inside a rounded box similar to the proposal 2 design (╭╮╰╯ corners)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:e64f6909-2f82-4eae-b46d-0074b7c4d711
  - session:9de494e6-e783-4785-ae67-1f7014dadd5d
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Article Embed Rendering

## Article Embed Rendering

The article embed component is rendered inside a rounded box similar to the proposal 2 design (╭╮╰╯ corners). The DefaultArticleRenderer renders the article embed as a rounded ╭─╮ box around the card. ArticleCard uses a horizontal medium layout with an 80x80px hero image on the left and title/author/date/read-time/summary on the right when a hero_image_url is present. It falls back to a vertical stacked layout when no hero image is present. Desktop embed-article renders title, byline, and summary but no hero image (the iced ArticleCard does not load images yet); this limitation is stated plainly in the registry entry. The embed card has a fixed height of 5 lines. The embed has additional left padding (indentation) to visually separate it from surrounding post content. Article hero images are fetched via ureq, decoded with the image crate, and cached as egui textures. [^9de49-2]

<!-- citations: [^9de49-2] [^e64f6-1] [^9de49-1] [^6a951-1] -->
## Title and Summary

The article embed title is bold with +1 left inset. The article embed summary is displayed in muted grey. [^e64f6-2]

## Byline and Reading Time

The article embed byline displays the author name and date in the style of proposal 5 (● Author · Date format). The byline displays as '● Author · Date · read time' with a red dot, light author, and dim date and read time. The article embed component includes an estimated reading time indication (e.g. '2 min read'). [^e64f6-3]
## See Also

