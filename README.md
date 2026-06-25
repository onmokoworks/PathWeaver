# ThreadType

ThreadType is a browser-based experimental Latin glyph editor built with React, TypeScript, Vite, and SVG.

The MVP focuses on the uppercase glyph `A`. The user draws one contour line. The start point can snap only to the top/right/bottom/left of a guide circle; while drawing, points weakly project to nearby circle circumferences. The path never closes automatically, and fill appears only when the user explicitly closes it.

## Run

```bash
npm install
npm run dev
```

## Build

```bash
npm run build
```

## MVP Features

- 1000 x 1000 virtual glyph coordinate system
- Baseline, cap height, x-height, sidebearing, and glyph frame guides
- Dashed guide circles with no fill
- Adjustable grid spacing, circle radius, weak circumference snap radius, and grid visibility
- Start snapping only at the four cardinal points of each circle
- Weak circle-edge projection while drawing
- One active contour line while drawing
- No automatic closing
- Fill appears only after explicit close
- Debug display for mode and current snap target
- Multiple contours per glyph
- Add and subtract contour operations
- Select, hide, show, delete, and clear contours
- Undo and redo for completed actions
- Japanese editor UI
- Export accumulated glyph contours as SVG
- Save and load full project JSON
