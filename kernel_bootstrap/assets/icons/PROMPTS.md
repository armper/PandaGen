# Where the icons came from (GFX-094)

Generated with OpenAI `gpt-image-2` at 1024x1024, one per app, then
reduced to the 256px sources in `src/` and exported by
`tools/art/icons.py` (squircle mask, 64/40/32/20/16 px, straight-alpha
RGBA). Regenerate a source with the shared style and its subject, then
run the tool.

Shared style:

> A single modern app icon, perfectly centred, filling the ENTIRE square
> canvas edge to edge with its background (no margin, no rounded corners,
> no drop shadow, no border, no text, no letters). Background: a rich,
> tasteful, smooth diagonal gradient with a soft inner light from the top.
> Foreground: one simple, bold, clean glyph in a soft 3D style with gentle
> shading, occupying about 55% of the canvas, centred. Consistent with a
> premium macOS/iOS-style icon family; crisp, high contrast, readable when
> shrunk to 16 pixels.

Subjects:

| Icon | Subject |
|---|---|
| notepad | a white paper note page with a folded top-right corner and four grey text lines, on a warm golden-yellow gradient |
| files | a blue folder with two white documents peeking out of it, on a bright azure-to-royal-blue gradient |
| terminal | a white '>' chevron followed by a short white underscore, on a dark graphite gradient |
| look | an artist's paint palette with five colourful paint dabs, on a violet-to-hot-pink gradient |
| calculator | a calculator with a dark display and a 3x3 grid of cream keys and one orange key, on an orange gradient |
| calendar | a white calendar page with a red top band, two ring binders and a grid of grey day squares, on a light warm grey gradient |
| timer | a white stopwatch with a single hand and a crown button, on a teal gradient |
| tiles | four rounded squares in a 2x2 grid coloured orange, blue, green and red, on a golden-yellow gradient |
| tasks | a checklist of three lines with white tick marks and a round green check badge, on a green gradient |
| sketch | a white pencil drawn diagonally with a pink eraser, on a coral-to-red gradient |
| panda | the friendly face of a cute panda, front view, on a deep midnight-navy gradient |

The desk's own icons -- notices (a bell), now (a gauge), shortcuts (a
keyboard) and bin -- are not generated: `tools/art/drawn_icons.py` draws
them in the same style (GFX-107).
