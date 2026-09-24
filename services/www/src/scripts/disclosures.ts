/** Let dragging select disclosure labels without also toggling their open state. */
let press:
  { element: Element; x: number; y: number; dragged: boolean } | undefined;

/** Find the disclosure controlled by a pointer event, including nested labels. */
const target = (event: Event) =>
  event.target instanceof Element
    ? event.target.closest(".ulo-site summary, .ulo-site button[aria-expanded]")
    : null;

document.addEventListener(
  "pointerdown",
  (event) => {
    const element = target(event);
    press =
      element && event.button === 0 && event.pointerType === "mouse"
        ? { element, x: event.clientX, y: event.clientY, dragged: false }
        : undefined;
  },
  true,
);
document.addEventListener(
  "pointermove",
  (event) => {
    if (
      press &&
      Math.hypot(event.clientX - press.x, event.clientY - press.y) > 4
    )
      press.dragged = true;
  },
  true,
);
document.addEventListener(
  "mousedown",
  (event) => {
    if (target(event) && event.detail > 1) event.preventDefault();
  },
  true,
);
document.addEventListener(
  "click",
  (event) => {
    const element = target(event);
    if (!element || !press || event.detail === 0 || element !== press.element)
      return;
    const selection = window.getSelection();
    if (press.dragged && selection && !selection.isCollapsed) {
      event.preventDefault();
      event.stopPropagation();
    } else if (
      !press.dragged &&
      selection?.anchorNode &&
      element.contains(selection.anchorNode)
    ) {
      selection.removeAllRanges();
    }
    press = undefined;
  },
  true,
);
