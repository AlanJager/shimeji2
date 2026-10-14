# Group strip

Image 1 is the CHARACTER reference — copy identity exactly: face, proportions,
palette, outline weight, outfit, style. Identity: {identity}.

Image 2 is a POSE GUIDE of {N} grey mannequins in {N} slots — copy each
mannequin's body pose and its position inside its slot ONLY. Do not copy the
mannequin's look, the slot boxes, numbers, or guide colors.

Draw exactly {N} frames of the character in one left-to-right row, one per
equal-width slot, the same scale and the same pixel size in every frame. The
character faces the viewer's LEFT unless a slot says otherwise. Transfer poses
WITHOUT MIRRORING (left and right are the character's own anatomical sides).

Slot notes:
{for each frame: "slot k — {frame description}; {contact rule}"}

{background_clause}

No ground, no shadows, no dust, no speed lines, no motion marks, no sparkles,
no glow, no afterimages, no text, and no detached effects. Crisp edges.
Nothing crosses a slot boundary. Frames that form one cycle (a walk, a head
turn) must not be copies of each other.

`{background_clause}` defaults to: solid flat pure green background (#00FF00).

# Single-frame repair

Image 1 is the CHARACTER reference. Image 2 is the current strip, for context.
Image 3 is the pose guide for slot {k}.

Redraw ONLY frame {k}: {description}; {contact rule}. Match the scale and the
style of the other frames in Image 2 exactly. Output one frame.
{background_clause} No effects or text.
